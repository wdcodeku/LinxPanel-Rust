use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub role: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: UserInfo,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: i64,
    pub username: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64,
    pub username: String,
    pub role: String,
    pub exp: usize,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Site {
    pub id: i64,
    pub name: String,
    pub domains: String,
    pub doc_root: String,
    pub site_type: String,
    pub index_files: String,
    pub status: String,
    pub ssl_enabled: bool,
    pub ssl_cert_path: Option<String>,
    pub ssl_key_path: Option<String>,
    pub proxy_pass: Option<String>,
    pub stopped_page_html: Option<String>,
    pub extra_config: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateSiteRequest {
    pub name: String,
    pub domains: Vec<String>,
    pub doc_root: Option<String>,
    pub site_type: Option<String>,
    pub proxy_pass: Option<String>,
    pub index_files: Option<String>,
    pub stopped_page_html: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSiteRequest {
    pub name: Option<String>,
    pub domains: Option<Vec<String>>,
    pub doc_root: Option<String>,
    pub site_type: Option<String>,
    pub proxy_pass: Option<String>,
    pub index_files: Option<String>,
    pub stopped_page_html: Option<String>,
    pub extra_config: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Domain {
    pub id: i64,
    pub site_id: i64,
    pub domain: String,
    pub is_primary: bool,
    pub sort_order: i64,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct AddDomainRequest {
    pub domain: String,
    pub is_primary: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateDomainRequest {
    pub domain: String,
}

#[derive(Debug, Deserialize)]
pub struct ReorderDomainsRequest {
    pub ordered_ids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub username: String,
    pub old_password: String,
    pub new_password: String,
}

#[derive(Debug, Serialize)]
pub struct CpuInfo {
    pub model_name: String,
    pub physical_cores: usize,
    pub logical_cores: usize,
    pub frequency_mhz: u64,
    pub vendor: String,
    pub usage_percent: f32,
    pub per_core_usage: Vec<f32>,
}

#[derive(Debug, Serialize)]
pub struct MemoryInfo {
    pub total: u64,
    pub used: u64,
    pub free: u64,
    pub available: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub usage_percent: f64,
}

#[derive(Debug, Serialize)]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: String,
    pub fs_type: String,
    pub total: u64,
    pub used: u64,
    pub free: u64,
    pub usage_percent: f64,
    pub is_removable: bool,
}

#[derive(Debug, Serialize)]
pub struct NetworkInfo {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub mac_address: String,
}

#[derive(Debug, Serialize)]
pub struct ProcessInfo {
    pub pid: String,
    pub name: String,
    pub cpu_percent: f32,
    pub memory: u64,
    pub virtual_memory: u64,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct PlatformInfo {
    pub hostname: String,
    pub os_name: String,
    pub os_version: String,
    pub kernel_version: String,
    pub arch: String,
    pub platform: String,
    pub uptime: u64,
    pub boot_time: u64,
    pub load_avg: [f64; 3],
}

#[derive(Debug, Serialize)]
pub struct SystemInfo {
    pub platform: PlatformInfo,
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub disks: Vec<DiskInfo>,
    pub networks: Vec<NetworkInfo>,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub success: bool,
    pub message: String,
    pub data: Option<T>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            message: "ok".into(),
            data: Some(data),
        }
    }

    pub fn ok_msg(msg: &str) -> Self {
        Self {
            success: true,
            message: msg.into(),
            data: None,
        }
    }

    pub fn err(msg: &str) -> Self {
        Self {
            success: false,
            message: msg.into(),
            data: None,
        }
    }
}
