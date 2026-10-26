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
    /// The user's `session_version` at login; tokens from before a password change
    /// carry an older value and are rejected. Absent in pre-versioning tokens.
    #[serde(default)]
    pub ver: i64,
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
    /// Set for app-backed site types; the site proxies to this application's port.
    pub app_id: Option<i64>,
    /// Raw nginx rewrite directives (the "pseudo-static" rules).
    pub rewrite_rules: Option<String>,
    /// Redirect plain HTTP to HTTPS (only meaningful with SSL enabled).
    pub force_https: bool,
    /// Emit Strict-Transport-Security on the TLS server block.
    pub hsts: bool,
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
    pub app_id: Option<i64>,
    pub rewrite_rules: Option<String>,
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
    pub app_id: Option<i64>,
    pub rewrite_rules: Option<String>,
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
    /// Ignored: the account is taken from the session. Kept for older clients.
    #[serde(default)]
    #[allow(dead_code)]
    pub username: String,
    pub old_password: String,
    pub new_password: String,
}

#[derive(Debug, Clone, Serialize)]
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

// ===== Realtime metrics =====

#[derive(Debug, Clone, Serialize)]
pub struct StaticInfo {
    pub hostname: String,
    pub os_name: String,
    pub os_version: String,
    pub kernel_version: String,
    pub arch: String,
    pub platform: String,
    pub boot_time: u64,
    pub cpu_model: String,
    pub cpu_vendor: String,
    pub cpu_frequency_mhz: u64,
    pub physical_cores: usize,
    pub logical_cores: usize,
    pub mem_total: u64,
    pub swap_total: u64,
    pub panel_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MetricsSample {
    pub ts: u64,
    pub cpu_percent: f32,
    pub cpu_cores: Vec<f32>,
    pub load_avg: [f64; 3],
    pub mem_total: u64,
    pub mem_used: u64,
    pub mem_available: u64,
    pub mem_percent: f64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub net_rx_rate: u64,
    pub net_tx_rate: u64,
    pub disk_read_rate: u64,
    pub disk_write_rate: u64,
    pub uptime: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetworkInterface {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_rate: u64,
    pub tx_rate: u64,
    pub mac_address: String,
    pub ip_addresses: Vec<String>,
    pub mtu: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MetricsDetail {
    pub ts: u64,
    pub disks: Vec<DiskInfo>,
    pub interfaces: Vec<NetworkInterface>,
    pub process_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum MetricsFrame {
    Tick(MetricsSample),
    Detail(MetricsDetail),
}

#[derive(Debug, Clone, Serialize)]
pub struct MetricsSnapshot {
    pub static_info: StaticInfo,
    pub history: Vec<MetricsSample>,
    pub latest: Option<MetricsSample>,
    pub detail: Option<MetricsDetail>,
}

// ===== Processes =====

#[derive(Debug, Clone, Deserialize)]
pub struct ProcessQuery {
    #[serde(default)]
    pub keyword: String,
    #[serde(default = "default_sort_by")]
    pub sort_by: String,
    #[serde(default = "default_order")]
    pub order: String,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_sort_by() -> String { "cpu".into() }
fn default_order() -> String { "desc".into() }
fn default_limit() -> usize { 50 }

impl Default for ProcessQuery {
    fn default() -> Self {
        Self {
            keyword: String::new(),
            sort_by: default_sort_by(),
            order: default_order(),
            offset: 0,
            limit: default_limit(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessRow {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub cmd: String,
    pub exe: String,
    pub user: String,
    pub cpu_percent: f32,
    pub memory: u64,
    pub virtual_memory: u64,
    pub status: String,
    pub run_time: u64,
    pub start_time: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessListResponse {
    pub total: usize,
    pub rows: Vec<ProcessRow>,
}

#[derive(Debug, Deserialize)]
pub struct KillProcessRequest {
    #[serde(default)]
    pub signal: String,
}
