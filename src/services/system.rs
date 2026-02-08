use crate::models::*;
use sysinfo::{System, Networks};

pub fn get_system_info() -> SystemInfo {
    let mut sys = System::new_all();
    sys.refresh_all();
    // second sample for accurate CPU
    std::thread::sleep(std::time::Duration::from_millis(200));
    sys.refresh_cpu_all();

    // Platform
    let load = System::load_average();
    let platform = PlatformInfo {
        hostname: System::host_name().unwrap_or_default(),
        os_name: System::name().unwrap_or_else(|| std::env::consts::OS.into()),
        os_version: System::long_os_version().unwrap_or_default(),
        kernel_version: System::kernel_version().unwrap_or_default(),
        arch: std::env::consts::ARCH.to_string(),
        platform: format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH),
        uptime: System::uptime(),
        boot_time: System::boot_time(),
        load_avg: [load.one, load.five, load.fifteen],
    };

    // CPU
    let cpus = sys.cpus();
    let model_name = cpus.first().map(|c| c.brand().to_string()).unwrap_or_default();
    let vendor = cpus.first().map(|c| c.vendor_id().to_string()).unwrap_or_default();
    let frequency = cpus.first().map(|c| c.frequency()).unwrap_or(0);
    let per_core: Vec<f32> = cpus.iter().map(|c| c.cpu_usage()).collect();
    let physical = sys.physical_core_count().unwrap_or(cpus.len());

    let cpu = CpuInfo {
        model_name,
        physical_cores: physical,
        logical_cores: cpus.len(),
        frequency_mhz: frequency,
        vendor,
        usage_percent: sys.global_cpu_usage(),
        per_core_usage: per_core,
    };

    // Memory
    let mem_total = sys.total_memory();
    let mem_used = sys.used_memory();
    let mem_free = sys.free_memory();
    let mem_available = sys.available_memory();
    let swap_total = sys.total_swap();
    let swap_used = sys.used_swap();
    let memory = MemoryInfo {
        total: mem_total,
        used: mem_used,
        free: mem_free,
        available: mem_available,
        swap_total,
        swap_used,
        usage_percent: if mem_total > 0 { (mem_used as f64 / mem_total as f64) * 100.0 } else { 0.0 },
    };

    // Disks
    let disk_list = sysinfo::Disks::new_with_refreshed_list();
    let disks: Vec<DiskInfo> = disk_list.iter().map(|d| {
        let total = d.total_space();
        let free = d.available_space();
        let used = total.saturating_sub(free);
        DiskInfo {
            name: d.name().to_string_lossy().to_string(),
            mount_point: d.mount_point().to_string_lossy().to_string(),
            fs_type: d.file_system().to_string_lossy().to_string(),
            total,
            used,
            free,
            usage_percent: if total > 0 { (used as f64 / total as f64) * 100.0 } else { 0.0 },
            is_removable: d.is_removable(),
        }
    }).collect();

    // Networks
    let nets = Networks::new_with_refreshed_list();
    let networks: Vec<NetworkInfo> = nets.iter().map(|(name, data)| {
        NetworkInfo {
            name: name.clone(),
            rx_bytes: data.total_received(),
            tx_bytes: data.total_transmitted(),
            rx_packets: data.total_packets_received(),
            tx_packets: data.total_packets_transmitted(),
            mac_address: data.mac_address().to_string(),
        }
    }).collect();

    SystemInfo { platform, cpu, memory, disks, networks }
}