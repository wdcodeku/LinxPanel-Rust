use crate::models::*;
use std::collections::VecDeque;
use std::cmp::Ordering;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
#[cfg(target_os = "windows")]
use std::time::Duration;
use sysinfo::{
    Networks, ProcessRefreshKind, ProcessesToUpdate, System, MINIMUM_CPU_UPDATE_INTERVAL,
};
#[cfg(target_os = "windows")]
use serde::Deserialize;
#[cfg(target_os = "windows")]
use std::process::Command;

const CPU_SMOOTH_WINDOW: usize = 4;
#[cfg(target_os = "windows")]
const WINDOWS_COUNTER_CACHE_TTL: Duration = Duration::from_millis(1500);

#[cfg(target_os = "windows")]
#[derive(Debug, Clone)]
struct WindowsCpuMetrics {
    usage_percent: f32,
    performance_percent: Option<f32>,
    frequency_mhz: Option<f32>,
    per_core_usage: Vec<f32>,
}

struct SystemSampler {
    sys: System,
    last_cpu_refresh: Option<Instant>,
    last_process_refresh: Option<Instant>,
    last_load_update: Option<Instant>,
    synthetic_load: [f64; 3],
    cpu_usage_history: VecDeque<f32>,
    per_core_usage_history: VecDeque<Vec<f32>>,
    #[cfg(target_os = "windows")]
    last_windows_counter_refresh: Option<Instant>,
    #[cfg(target_os = "windows")]
    last_windows_counter_value: Option<WindowsCpuMetrics>,
}

static SYSTEM_SAMPLER: OnceLock<Mutex<SystemSampler>> = OnceLock::new();

fn sampler() -> &'static Mutex<SystemSampler> {
    SYSTEM_SAMPLER.get_or_init(|| {
        let mut sys = System::new_all();
        sys.refresh_memory();
        sys.refresh_cpu_all();
        Mutex::new(SystemSampler {
            sys,
            last_cpu_refresh: Some(Instant::now()),
            last_process_refresh: Some(Instant::now()),
            last_load_update: None,
            synthetic_load: [0.0, 0.0, 0.0],
            cpu_usage_history: VecDeque::new(),
            per_core_usage_history: VecDeque::new(),
            #[cfg(target_os = "windows")]
            last_windows_counter_refresh: None,
            #[cfg(target_os = "windows")]
            last_windows_counter_value: None,
        })
    })
}

fn update_synthetic_load(
    old: [f64; 3],
    cpu_usage_percent: f32,
    logical_cores: usize,
    dt_seconds: f64,
) -> [f64; 3] {
    let instant_load = (cpu_usage_percent as f64 / 100.0) * logical_cores.max(1) as f64;
    if dt_seconds <= 0.0 || old.iter().all(|value| *value == 0.0) {
        return [instant_load, instant_load, instant_load];
    }

    let ewma = |value: f64, tau: f64| {
        let decay = (-dt_seconds / tau).exp();
        value * decay + instant_load * (1.0 - decay)
    };

    [ewma(old[0], 60.0), ewma(old[1], 300.0), ewma(old[2], 900.0)]
}

fn clamp_percent(value: f32) -> f32 {
    if !value.is_finite() {
        0.0
    } else {
        value.clamp(0.0, 100.0)
    }
}

fn normalize_per_core_usage(
    per_core_usage: Vec<f32>,
    global_usage_percent: f32,
    logical_cores: usize,
) -> Vec<f32> {
    let mut normalized = per_core_usage
        .into_iter()
        .map(clamp_percent)
        .collect::<Vec<f32>>();
    if normalized.is_empty() || logical_cores <= 1 {
        return normalized;
    }

    let global = clamp_percent(global_usage_percent);
    let sum = normalized.iter().copied().sum::<f32>();
    let avg = sum / logical_cores as f32;

    let looks_like_total_share = global >= 4.0
        && sum <= 120.0
        && (sum - global).abs() <= global.max(5.0) * 0.65
        && avg < global * 0.35;

    if looks_like_total_share {
        let factor = logical_cores as f32;
        for value in &mut normalized {
            *value = clamp_percent(*value * factor);
        }
    }

    normalized
}

fn push_cpu_sample(sampler: &mut SystemSampler, usage_percent: f32, per_core_usage: &[f32]) {
    if sampler.cpu_usage_history.len() >= CPU_SMOOTH_WINDOW {
        sampler.cpu_usage_history.pop_front();
    }
    sampler
        .cpu_usage_history
        .push_back(clamp_percent(usage_percent));

    if per_core_usage.is_empty() {
        sampler.per_core_usage_history.clear();
        return;
    }

    let sample = per_core_usage
        .iter()
        .copied()
        .map(clamp_percent)
        .collect::<Vec<f32>>();
    let mismatched_core_count = sampler
        .per_core_usage_history
        .back()
        .map(|previous| previous.len() != sample.len())
        .unwrap_or(false);
    if mismatched_core_count {
        sampler.per_core_usage_history.clear();
    }

    if sampler.per_core_usage_history.len() >= CPU_SMOOTH_WINDOW {
        sampler.per_core_usage_history.pop_front();
    }
    sampler.per_core_usage_history.push_back(sample);
}

fn smoothed_cpu_sample(sampler: &SystemSampler) -> Option<(f32, Vec<f32>)> {
    if sampler.cpu_usage_history.is_empty() {
        return None;
    }

    let total_usage = sampler.cpu_usage_history.iter().copied().sum::<f32>();
    let avg_usage = clamp_percent(total_usage / sampler.cpu_usage_history.len() as f32);

    if sampler.per_core_usage_history.is_empty() {
        return Some((avg_usage, Vec::new()));
    }

    let core_len = sampler
        .per_core_usage_history
        .front()
        .map(|row| row.len())
        .unwrap_or(0);
    if core_len == 0 {
        return Some((avg_usage, Vec::new()));
    }

    let mut sums = vec![0.0_f32; core_len];
    for row in &sampler.per_core_usage_history {
        for (index, value) in row.iter().take(core_len).enumerate() {
            sums[index] += *value;
        }
    }
    let samples = sampler.per_core_usage_history.len() as f32;
    let per_core = sums
        .into_iter()
        .map(|value| clamp_percent(value / samples))
        .collect::<Vec<f32>>();
    Some((avg_usage, per_core))
}

#[cfg(target_os = "windows")]
#[derive(Debug, Deserialize)]
struct WindowsCpuCounterPayload {
    utility: Option<f64>,
    performance: Option<f64>,
    frequency: Option<f64>,
    cores: Option<Vec<f64>>,
}

#[cfg(target_os = "windows")]
fn read_windows_cpu_metrics() -> Option<WindowsCpuMetrics> {
    let script = r#"
$sample = Get-Counter '\Processor Information(*)\% Processor Utility','\Processor Information(*)\% Processor Performance','\Processor Information(*)\Processor Frequency'
$samples = $sample.CounterSamples

$utility = ($samples | Where-Object {
  $_.InstanceName -ieq '_Total' -and $_.Path -match '% processor utility$'
} | Select-Object -First 1).CookedValue

$performance = ($samples | Where-Object {
  $_.InstanceName -ieq '_Total' -and $_.Path -match '% processor performance$'
} | Select-Object -First 1).CookedValue

$frequency = ($samples | Where-Object {
  $_.InstanceName -ieq '_Total' -and $_.Path -match 'processor frequency$'
} | Select-Object -First 1).CookedValue

$cores = $samples |
  Where-Object {
    $_.InstanceName -ine '_Total' -and
    ($_.InstanceName -match '^\d+$' -or $_.InstanceName -match '^\d+,\d+$') -and
    $_.Path -match '% processor utility$'
  } |
  Sort-Object {
    $name = $_.InstanceName
    if ($name -match '^\d+,(\d+)$') {
      [int]$Matches[1]
    } elseif ($name -match '^\d+$') {
      [int]$name
    } else {
      [int]::MaxValue
    }
  } |
  ForEach-Object { [double]$_.CookedValue }

@{
  utility = [double]$utility
  performance = [double]$performance
  frequency = [double]$frequency
  cores = $cores
} | ConvertTo-Json -Compress
"#;

    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let payload: WindowsCpuCounterPayload = serde_json::from_str(text.trim()).ok()?;
    let usage_percent = clamp_percent(payload.utility.unwrap_or(0.0) as f32);
    let performance_percent = payload.performance.map(|value| value as f32);
    let frequency_mhz = payload.frequency.map(|value| value as f32);
    let per_core_usage = payload
        .cores
        .unwrap_or_default()
        .into_iter()
        .map(|value| clamp_percent(value as f32))
        .collect::<Vec<f32>>();
    Some(WindowsCpuMetrics {
        usage_percent,
        performance_percent,
        frequency_mhz,
        per_core_usage,
    })
}

#[cfg(target_os = "windows")]
fn read_windows_cpu_metrics_cached(
    sampler: &mut SystemSampler,
    now: Instant,
) -> Option<WindowsCpuMetrics> {
    if let (Some(cached_at), Some(value)) = (
        sampler.last_windows_counter_refresh,
        sampler.last_windows_counter_value.as_ref(),
    ) {
        if now.duration_since(cached_at) < WINDOWS_COUNTER_CACHE_TTL {
            return Some(value.clone());
        }
    }

    let fresh = read_windows_cpu_metrics()?;
    sampler.last_windows_counter_refresh = Some(now);
    sampler.last_windows_counter_value = Some(fresh.clone());
    Some(fresh)
}

pub fn get_system_info() -> SystemInfo {
    let mut guard = sampler().lock().unwrap_or_else(|err| err.into_inner());
    let now = Instant::now();

    let allow_cpu_refresh = guard
        .last_cpu_refresh
        .map(|last| now.duration_since(last) >= MINIMUM_CPU_UPDATE_INTERVAL)
        .unwrap_or(true);
    let dt_seconds = guard
        .last_load_update
        .map(|last| now.duration_since(last).as_secs_f64().max(0.001))
        .unwrap_or(0.0);

    {
        let sys = &mut guard.sys;
        sys.refresh_memory();
        if allow_cpu_refresh {
            sys.refresh_cpu_usage();
            sys.refresh_cpu_frequency();
            guard.last_cpu_refresh = Some(now);
        }
    }

    let mut cpu = {
        let sys = &guard.sys;
        let cpus = sys.cpus();
        let model_name = cpus
            .first()
            .map(|cpu| cpu.brand().to_string())
            .unwrap_or_default();
        let vendor = cpus
            .first()
            .map(|cpu| cpu.vendor_id().to_string())
            .unwrap_or_default();
        let frequency = cpus.first().map(|cpu| cpu.frequency()).unwrap_or(0);
        let raw_per_core_usage: Vec<f32> = cpus.iter().map(|cpu| cpu.cpu_usage()).collect();
        let logical_cores = cpus.len().max(1);
        let usage_percent = clamp_percent(sys.global_cpu_usage());
        let per_core_usage =
            normalize_per_core_usage(raw_per_core_usage, usage_percent, logical_cores);

        CpuInfo {
            model_name,
            physical_cores: sys.physical_core_count().unwrap_or(logical_cores),
            logical_cores,
            frequency_mhz: frequency,
            vendor,
            usage_percent,
            per_core_usage,
        }
    };

    #[cfg(target_os = "windows")]
    {
        let base_frequency_mhz = cpu.frequency_mhz;
        if let Some(metrics) = read_windows_cpu_metrics_cached(&mut guard, now) {
            cpu.usage_percent = clamp_percent(metrics.usage_percent);
            if metrics.per_core_usage.len() == cpu.logical_cores && !metrics.per_core_usage.is_empty() {
                cpu.per_core_usage = metrics
                    .per_core_usage
                    .iter()
                    .copied()
                    .map(clamp_percent)
                    .collect::<Vec<f32>>();
            }

            let mut computed_frequency = None;
            if let Some(performance_percent) = metrics.performance_percent {
                if base_frequency_mhz > 0 {
                    let value = (base_frequency_mhz as f32) * (performance_percent / 100.0);
                    if value.is_finite() && value > 1.0 {
                        computed_frequency = Some(value.round().clamp(1.0, 20000.0) as u64);
                    }
                }
            }
            if computed_frequency.is_none() {
                if let Some(counter_frequency) = metrics.frequency_mhz {
                    if counter_frequency.is_finite() && counter_frequency > 1.0 {
                        computed_frequency = Some(counter_frequency.round().clamp(1.0, 20000.0) as u64);
                    }
                }
            }
            if let Some(frequency_mhz) = computed_frequency {
                cpu.frequency_mhz = frequency_mhz;
            }
        }
    }

    if allow_cpu_refresh || guard.cpu_usage_history.is_empty() {
        push_cpu_sample(&mut guard, cpu.usage_percent, &cpu.per_core_usage);
    }
    if let Some((usage, per_core)) = smoothed_cpu_sample(&guard) {
        cpu.usage_percent = usage;
        if per_core.len() == cpu.logical_cores && !per_core.is_empty() {
            cpu.per_core_usage = per_core;
        }
    }

    guard.synthetic_load = update_synthetic_load(
        guard.synthetic_load,
        cpu.usage_percent,
        cpu.logical_cores,
        dt_seconds,
    );
    guard.last_load_update = Some(now);

    let native_load = {
        let load = System::load_average();
        [load.one, load.five, load.fifteen]
    };
    let native_load_supported = native_load.iter().any(|value| *value > 0.0);
    let load_avg = if cfg!(target_os = "windows") || !native_load_supported {
        guard.synthetic_load
    } else {
        native_load
    };

    let platform = PlatformInfo {
        hostname: System::host_name().unwrap_or_default(),
        os_name: System::name().unwrap_or_else(|| std::env::consts::OS.into()),
        os_version: System::long_os_version().unwrap_or_default(),
        kernel_version: System::kernel_version().unwrap_or_default(),
        arch: std::env::consts::ARCH.to_string(),
        platform: format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH),
        uptime: System::uptime(),
        boot_time: System::boot_time(),
        load_avg,
    };

    let memory = {
        let sys = &guard.sys;
        let total = sys.total_memory();
        let used = sys.used_memory();
        MemoryInfo {
            total,
            used,
            free: sys.free_memory(),
            available: sys.available_memory(),
            swap_total: sys.total_swap(),
            swap_used: sys.used_swap(),
            usage_percent: if total > 0 {
                (used as f64 / total as f64) * 100.0
            } else {
                0.0
            },
        }
    };

    let disk_list = sysinfo::Disks::new_with_refreshed_list();
    let disks: Vec<DiskInfo> = disk_list
        .iter()
        .map(|disk| {
            let total = disk.total_space();
            let free = disk.available_space();
            let used = total.saturating_sub(free);
            DiskInfo {
                name: disk.name().to_string_lossy().to_string(),
                mount_point: disk.mount_point().to_string_lossy().to_string(),
                fs_type: disk.file_system().to_string_lossy().to_string(),
                total,
                used,
                free,
                usage_percent: if total > 0 {
                    (used as f64 / total as f64) * 100.0
                } else {
                    0.0
                },
                is_removable: disk.is_removable(),
            }
        })
        .collect();

    let nets = Networks::new_with_refreshed_list();
    let networks: Vec<NetworkInfo> = nets
        .iter()
        .map(|(name, data)| NetworkInfo {
            name: name.clone(),
            rx_bytes: data.total_received(),
            tx_bytes: data.total_transmitted(),
            rx_packets: data.total_packets_received(),
            tx_packets: data.total_packets_transmitted(),
            mac_address: data.mac_address().to_string(),
        })
        .collect();

    SystemInfo {
        platform,
        cpu,
        memory,
        disks,
        networks,
    }
}

pub fn get_top_processes(limit: usize) -> Vec<ProcessInfo> {
    let top_n = limit.clamp(1, 64);
    let mut guard = sampler().lock().unwrap_or_else(|err| err.into_inner());
    let now = Instant::now();

    let allow_process_cpu_refresh = guard
        .last_process_refresh
        .map(|last| now.duration_since(last) >= MINIMUM_CPU_UPDATE_INTERVAL)
        .unwrap_or(true);

    let mut refresh_kind = ProcessRefreshKind::nothing().with_memory();
    if allow_process_cpu_refresh {
        refresh_kind = refresh_kind.with_cpu();
        guard.last_process_refresh = Some(now);
    }

    guard
        .sys
        .refresh_processes_specifics(ProcessesToUpdate::All, true, refresh_kind);

    let mut processes: Vec<ProcessInfo> = guard
        .sys
        .processes()
        .values()
        .map(|process| ProcessInfo {
            pid: process.pid().to_string(),
            name: process.name().to_string_lossy().to_string(),
            cpu_percent: process.cpu_usage(),
            memory: process.memory(),
            virtual_memory: process.virtual_memory(),
            status: format!("{:?}", process.status()),
        })
        .filter(|process| process.cpu_percent > 0.01 || process.memory > 0)
        .collect();

    processes.sort_by(|a, b| {
        b.cpu_percent
            .partial_cmp(&a.cpu_percent)
            .unwrap_or(Ordering::Equal)
            .then_with(|| b.memory.cmp(&a.memory))
    });
    processes.truncate(top_n);
    processes
}
