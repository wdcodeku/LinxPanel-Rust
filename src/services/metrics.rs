//! Realtime metrics engine.
//!
//! A single background task owns one `System` instance and samples it on a fixed
//! cadence. Every sample is pushed into a ring buffer and broadcast to subscribers,
//! so N connected clients cost one sampling pass instead of N.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sysinfo::{
    DiskRefreshKind, Disks, MemoryRefreshKind, Networks, ProcessRefreshKind, ProcessesToUpdate,
    System,
};
use tokio::sync::broadcast;

use crate::models::*;

/// Sampling cadence for the cheap frame (cpu/memory/load/net/disk io).
pub const TICK_INTERVAL: Duration = Duration::from_millis(1000);
/// How many ticks between expensive refreshes (disk enumeration, process list).
const DETAIL_EVERY_N_TICKS: u32 = 5;
/// Ring buffer capacity: 10 minutes of 1s samples.
pub const HISTORY_CAPACITY: usize = 600;
/// Broadcast backlog. Slow clients lag rather than block the sampler.
const BROADCAST_CAPACITY: usize = 64;

pub struct MetricsEngine {
    state: RwLock<MetricsState>,
    tx: broadcast::Sender<MetricsFrame>,
}

struct MetricsState {
    static_info: StaticInfo,
    history: VecDeque<MetricsSample>,
    latest: Option<MetricsSample>,
    detail: Option<MetricsDetail>,
}

impl MetricsEngine {
    pub fn new() -> Arc<Self> {
        let (tx, _rx) = broadcast::channel(BROADCAST_CAPACITY);
        Arc::new(Self {
            state: RwLock::new(MetricsState {
                static_info: StaticInfo::collect(),
                history: VecDeque::with_capacity(HISTORY_CAPACITY),
                latest: None,
                detail: None,
            }),
            tx,
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<MetricsFrame> {
        self.tx.subscribe()
    }

    /// Everything a freshly connected client needs to render without waiting for a tick.
    pub fn snapshot(&self) -> MetricsSnapshot {
        let state = self.state.read().unwrap_or_else(|err| err.into_inner());
        MetricsSnapshot {
            static_info: state.static_info.clone(),
            history: state.history.iter().cloned().collect(),
            latest: state.latest.clone(),
            detail: state.detail.clone(),
        }
    }

    fn publish_sample(&self, sample: MetricsSample) {
        {
            let mut state = self.state.write().unwrap_or_else(|err| err.into_inner());
            if state.history.len() >= HISTORY_CAPACITY {
                state.history.pop_front();
            }
            state.history.push_back(sample.clone());
            state.latest = Some(sample.clone());
        }
        let _ = self.tx.send(MetricsFrame::Tick(sample));
    }

    fn publish_detail(&self, detail: MetricsDetail) {
        {
            let mut state = self.state.write().unwrap_or_else(|err| err.into_inner());
            state.detail = Some(detail.clone());
        }
        let _ = self.tx.send(MetricsFrame::Detail(detail));
    }
}

/// Spawns the sampling loop. Called once at startup.
///
/// `shutdown` lets the loop exit: it runs on the blocking pool, and dropping the
/// Tokio runtime waits for blocking tasks, so an endless loop would hang exit.
pub fn spawn(engine: Arc<MetricsEngine>, shutdown: tokio::sync::watch::Receiver<bool>) {
    tokio::task::spawn_blocking(move || sampler_loop(engine, shutdown));
    // Per-process CPU is a delta between two refreshes, so the first caller would
    // otherwise see zeros. Prime the baseline now, while nobody is waiting on it.
    tokio::task::spawn_blocking(|| {
        let _ = list_processes(&ProcessQuery::default());
    });
}

fn sampler_loop(engine: Arc<MetricsEngine>, shutdown: tokio::sync::watch::Receiver<bool>) {
    let mut sys = System::new();
    sys.refresh_cpu_all();
    sys.refresh_memory();

    let mut networks = Networks::new_with_refreshed_list();
    let mut disks = Disks::new_with_refreshed_list_specifics(
        DiskRefreshKind::nothing().with_storage().with_io_usage(),
    );

    let mut tick: u32 = 0;
    let mut last_tick_at = Instant::now();
    // Synthetic load average for platforms that do not expose one (Windows).
    let mut synthetic_load = [0f64; 3];

    loop {
        if *shutdown.borrow() {
            tracing::debug!("Metrics sampler stopping");
            return;
        }

        let started = Instant::now();
        let dt = started.duration_since(last_tick_at).as_secs_f64().max(0.001);
        last_tick_at = started;

        sys.refresh_cpu_usage();
        sys.refresh_memory_specifics(MemoryRefreshKind::everything());
        networks.refresh(false);
        disks.refresh_specifics(false, DiskRefreshKind::nothing().with_io_usage());

        let cpu_cores: Vec<f32> = sys.cpus().iter().map(|c| clamp_pct(c.cpu_usage())).collect();
        let cpu_percent = clamp_pct(sys.global_cpu_usage());

        // Aggregate the per-refresh deltas into rates.
        let (rx_delta, tx_delta) = networks
            .iter()
            .fold((0u64, 0u64), |(rx, tx), (_, data)| {
                (rx + data.received(), tx + data.transmitted())
            });
        let (read_delta, write_delta) = disks.list().iter().fold((0u64, 0u64), |(r, w), disk| {
            let usage = disk.usage();
            (r + usage.read_bytes, w + usage.written_bytes)
        });

        let load_avg = {
            let native = System::load_average();
            let native = [native.one, native.five, native.fifteen];
            if native.iter().any(|v| *v > 0.0) {
                native
            } else {
                synthetic_load =
                    advance_synthetic_load(synthetic_load, cpu_percent, cpu_cores.len().max(1), dt);
                synthetic_load
            }
        };

        let total_memory = sys.total_memory();
        let used_memory = sys.used_memory();

        let sample = MetricsSample {
            ts: epoch_millis(),
            cpu_percent,
            cpu_cores,
            load_avg,
            mem_total: total_memory,
            mem_used: used_memory,
            mem_available: sys.available_memory(),
            mem_percent: percent_of(used_memory, total_memory),
            swap_total: sys.total_swap(),
            swap_used: sys.used_swap(),
            net_rx_rate: rate(rx_delta, dt),
            net_tx_rate: rate(tx_delta, dt),
            disk_read_rate: rate(read_delta, dt),
            disk_write_rate: rate(write_delta, dt),
            uptime: System::uptime(),
        };
        engine.publish_sample(sample);

        if tick % DETAIL_EVERY_N_TICKS == 0 {
            disks.refresh_specifics(true, DiskRefreshKind::nothing().with_storage());
            sys.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing(),
            );
            engine.publish_detail(collect_detail(&sys, &networks, &disks, dt));
        }
        tick = tick.wrapping_add(1);

        // Hold the cadence regardless of how long sampling took, but wake often enough
        // to notice a shutdown request.
        let elapsed = started.elapsed();
        if elapsed < TICK_INTERVAL {
            let mut remaining = TICK_INTERVAL - elapsed;
            while remaining > Duration::ZERO {
                if *shutdown.borrow() {
                    return;
                }
                let slice = remaining.min(Duration::from_millis(100));
                std::thread::sleep(slice);
                remaining -= slice;
            }
        }
    }
}

fn collect_detail(
    sys: &System,
    networks: &Networks,
    disks: &Disks,
    dt: f64,
) -> MetricsDetail {
    let disk_rows: Vec<DiskInfo> = disks
        .list()
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
                usage_percent: percent_of(used, total),
                is_removable: disk.is_removable(),
            }
        })
        .collect();

    let interfaces: Vec<NetworkInterface> = networks
        .iter()
        .map(|(name, data)| NetworkInterface {
            name: name.clone(),
            rx_bytes: data.total_received(),
            tx_bytes: data.total_transmitted(),
            rx_rate: rate(data.received(), dt),
            tx_rate: rate(data.transmitted(), dt),
            mac_address: data.mac_address().to_string(),
            ip_addresses: data
                .ip_networks()
                .iter()
                .map(|net| net.addr.to_string())
                .collect(),
            mtu: data.mtu(),
        })
        .collect();

    MetricsDetail {
        ts: epoch_millis(),
        disks: disk_rows,
        interfaces,
        process_count: sys.processes().len(),
    }
}

fn advance_synthetic_load(old: [f64; 3], cpu_percent: f32, cores: usize, dt: f64) -> [f64; 3] {
    let instant = (cpu_percent as f64 / 100.0) * cores as f64;
    if old.iter().all(|v| *v == 0.0) {
        return [instant; 3];
    }
    let ewma = |value: f64, tau: f64| {
        let decay = (-dt / tau).exp();
        value * decay + instant * (1.0 - decay)
    };
    [ewma(old[0], 60.0), ewma(old[1], 300.0), ewma(old[2], 900.0)]
}

fn rate(delta: u64, dt: f64) -> u64 {
    if dt <= 0.0 {
        return 0;
    }
    (delta as f64 / dt).round().max(0.0) as u64
}

fn percent_of(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        (part as f64 / whole as f64) * 100.0
    }
}

fn clamp_pct(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 100.0)
    } else {
        0.0
    }
}

pub fn epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl StaticInfo {
    fn collect() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_all();
        sys.refresh_memory();
        let cpus = sys.cpus();
        Self {
            hostname: System::host_name().unwrap_or_default(),
            os_name: System::name().unwrap_or_else(|| std::env::consts::OS.into()),
            os_version: System::long_os_version().unwrap_or_default(),
            kernel_version: System::kernel_version().unwrap_or_default(),
            arch: std::env::consts::ARCH.to_string(),
            platform: format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH),
            boot_time: System::boot_time(),
            cpu_model: cpus.first().map(|c| c.brand().trim().to_string()).unwrap_or_default(),
            cpu_vendor: cpus.first().map(|c| c.vendor_id().to_string()).unwrap_or_default(),
            cpu_frequency_mhz: cpus.first().map(|c| c.frequency()).unwrap_or(0),
            physical_cores: sys.physical_core_count().unwrap_or_else(|| cpus.len()),
            logical_cores: cpus.len().max(1),
            mem_total: sys.total_memory(),
            swap_total: sys.total_swap(),
            panel_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Process listing is on-demand (it is the expensive one) and kept out of the tick loop.
pub fn list_processes(query: &ProcessQuery) -> ProcessListResponse {
    static PROC_SYS: std::sync::OnceLock<std::sync::Mutex<System>> = std::sync::OnceLock::new();
    let lock = PROC_SYS.get_or_init(|| std::sync::Mutex::new(System::new()));
    let mut sys = lock.lock().unwrap_or_else(|err| err.into_inner());

    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_user(sysinfo::UpdateKind::OnlyIfNotSet)
            .with_cmd(sysinfo::UpdateKind::OnlyIfNotSet)
            .with_exe(sysinfo::UpdateKind::OnlyIfNotSet),
    );

    let logical_cores = sys.cpus().len().max(1) as f32;
    let keyword = query.keyword.trim().to_lowercase();
    let users = sysinfo::Users::new_with_refreshed_list();

    let mut rows: Vec<ProcessRow> = sys
        .processes()
        .values()
        .map(|process| {
            let cmd = process
                .cmd()
                .iter()
                .map(|part| part.to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join(" ");
            ProcessRow {
                pid: process.pid().as_u32(),
                parent_pid: process.parent().map(|p| p.as_u32()),
                name: process.name().to_string_lossy().to_string(),
                cmd,
                exe: process
                    .exe()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
                user: process
                    .user_id()
                    .and_then(|uid| users.get_user_by_id(uid))
                    .map(|user| user.name().to_string())
                    .or_else(|| process.user_id().map(|uid| uid.to_string()))
                    .unwrap_or_default(),
                // sysinfo reports percent-of-one-core; normalise to percent-of-machine.
                // A short refresh interval can overshoot, so clamp rather than show >100%.
                cpu_percent: (process.cpu_usage() / logical_cores).clamp(0.0, 100.0),
                memory: process.memory(),
                virtual_memory: process.virtual_memory(),
                status: process.status().to_string(),
                run_time: process.run_time(),
                start_time: process.start_time(),
            }
        })
        .filter(|row| {
            if keyword.is_empty() {
                return true;
            }
            row.name.to_lowercase().contains(&keyword)
                || row.cmd.to_lowercase().contains(&keyword)
                || row.pid.to_string() == keyword
        })
        .collect();

    let total = rows.len();
    let descending = query.order.eq_ignore_ascii_case("desc");
    match query.sort_by.as_str() {
        "memory" => rows.sort_by(|a, b| a.memory.cmp(&b.memory)),
        "pid" => rows.sort_by(|a, b| a.pid.cmp(&b.pid)),
        "name" => rows.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        "run_time" => rows.sort_by(|a, b| a.run_time.cmp(&b.run_time)),
        _ => rows.sort_by(|a, b| {
            a.cpu_percent
                .partial_cmp(&b.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.memory.cmp(&b.memory))
        }),
    }
    if descending {
        rows.reverse();
    }

    let offset = query.offset.min(total);
    let rows = rows
        .into_iter()
        .skip(offset)
        .take(query.limit.clamp(1, 500))
        .collect();

    ProcessListResponse { total, rows }
}

/// Sends a signal to a process. Returns a human-readable error on failure.
pub fn signal_process(pid: u32, signal: &str) -> Result<(), String> {
    let mut sys = System::new();
    let sysinfo_pid = sysinfo::Pid::from_u32(pid);
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[sysinfo_pid]),
        true,
        ProcessRefreshKind::nothing(),
    );

    let process = sys
        .process(sysinfo_pid)
        .ok_or_else(|| format!("Process {} not found", pid))?;

    let sig = match signal.to_ascii_uppercase().as_str() {
        "TERM" | "SIGTERM" | "" => sysinfo::Signal::Term,
        "KILL" | "SIGKILL" => sysinfo::Signal::Kill,
        "HUP" | "SIGHUP" => sysinfo::Signal::Hangup,
        "INT" | "SIGINT" => sysinfo::Signal::Interrupt,
        "STOP" | "SIGSTOP" => sysinfo::Signal::Stop,
        "CONT" | "SIGCONT" => sysinfo::Signal::Continue,
        other => return Err(format!("Unsupported signal: {}", other)),
    };

    match process.kill_with(sig) {
        Some(true) => Ok(()),
        Some(false) => Err(format!("Failed to signal process {}", pid)),
        None => Err("Signal is not supported on this platform".into()),
    }
}

/// Aggregated per-device view used by the dashboard disk table.
pub fn merge_disks(disks: &[DiskInfo]) -> Vec<DiskInfo> {
    let mut merged: HashMap<String, DiskInfo> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for disk in disks {
        if disk.total == 0 || is_virtual_fs(&disk.fs_type) {
            continue;
        }
        let key = device_key(&disk.name, &disk.mount_point);
        let entry = merged.entry(key.clone()).or_insert_with(|| {
            order.push(key.clone());
            DiskInfo {
                name: key.clone(),
                mount_point: disk.mount_point.clone(),
                fs_type: disk.fs_type.clone(),
                total: 0,
                used: 0,
                free: 0,
                usage_percent: 0.0,
                is_removable: disk.is_removable,
            }
        });
        entry.total += disk.total;
        entry.used += disk.used;
        entry.free += disk.free;
        entry.usage_percent = percent_of(entry.used, entry.total);
    }

    order
        .into_iter()
        .filter_map(|key| merged.remove(&key))
        .collect()
}

fn is_virtual_fs(fs_type: &str) -> bool {
    const VIRTUAL: &[&str] = &[
        "overlay", "aufs", "tmpfs", "devtmpfs", "squashfs", "ramfs", "zram", "nsfs", "tracefs",
        "securityfs", "cgroup", "cgroup2", "autofs", "proc", "procfs", "sysfs", "devpts", "mqueue",
        "hugetlbfs", "fusectl", "pstore", "configfs", "debugfs", "rpc_pipefs", "bpf",
        "binfmt_misc",
    ];
    let lowered = fs_type.trim().to_lowercase();
    VIRTUAL.contains(&lowered.as_str())
}

fn device_key(name: &str, mount_point: &str) -> String {
    let value = name.trim();
    if value.is_empty() {
        return mount_point.to_string();
    }
    if let Some(rest) = value.strip_prefix("/dev/") {
        // Collapse partitions onto their parent device: nvme0n1p2 -> nvme0n1, sda1 -> sda.
        if let Some(stripped) = rest
            .strip_suffix(|c: char| c.is_ascii_digit())
            .filter(|_| rest.contains('p') && rest.starts_with("nvme"))
        {
            return format!("/dev/{}", stripped.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end_matches('p'));
        }
        let trimmed = rest.trim_end_matches(|c: char| c.is_ascii_digit());
        if !trimmed.is_empty() && trimmed != rest {
            return format!("/dev/{}", trimmed);
        }
    }
    value.to_string()
}
