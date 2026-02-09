# LinxPanel - Modern Linux Server Management Panel (Rust + Vue 3)

## 中文版

### Linux 一键自动部署

可直接使用以下命令自动安装依赖、构建并部署 LinxPanel：

- `bash -c "$(curl -sSL https://raw.githubusercontent.com/wdcodeku/LinxPanel-Rust/main/scripts/install.sh)"`

默认行为：

- 首个提示会让你选择语言（默认英文）
- 第二个提示会让你选择安装方式（默认二进制安装）
- 安装目录：`/opt/linxpanel`
- 监听端口：`7200`
- 自动创建并启动 `systemd` 服务（若系统支持）
- 若系统 `cargo` 过旧，会自动安装/切换到 `rustup stable`
- 安装结束会自动输出真实访问 IP（优先公网 IP）
- 自动安装命令行工具：`Lp`（别名：`lp`）
- 无交互部署可指定语言：
  - `LINXPANEL_LANG=zh bash -c "$(curl -sSL https://raw.githubusercontent.com/wdcodeku/LinxPanel-Rust/main/scripts/install.sh)"`
  - 指定安装方式：`LINXPANEL_INSTALL_MODE=binary` 或 `LINXPANEL_INSTALL_MODE=build`
  - 如需手动指定输出 IP：`LINXPANEL_PUBLIC_IP=1.2.3.4`

### Lp 命令行工具

- `Lp status`：查看服务状态
- `Lp start|stop|restart`：启停/重启服务
- `Lp logs [out|err|all|service]`：查看日志
- `Lp info`：查看安装路径、端口与访问地址
- `Lp update [binary|build]`：一键更新（默认二进制）

### 单文件部署（内嵌 Web）

LinxPanel 在构建时会将 `web/dist` 内嵌进 Rust 可执行文件，部署时只需要后端单个二进制文件。

- Windows:
  - `powershell -ExecutionPolicy Bypass -File .\scripts\build-single.ps1`
  - 输出：`target\release\linxpanel.exe`
- Linux:
  - `bash ./scripts/build-single.sh`
  - 输出：`target/release/linxpanel`
- 手动构建：
  - `cd web && npm ci && npm run build`
  - `cd .. && cargo build --release`

说明：

- 若 `web/dist` 缺失，构建仍会成功，但会内嵌占位页面。
- 生产环境请先执行前端构建，再执行 `cargo build --release`。

### OpenResty 实时落地

网站/域名修改会自动落地到 OpenResty：

- 按站点生成配置到 `sites-enabled`
- 先执行 `nginx -t` 再重载
- 仅在校验成功后重载 OpenResty
- 失败自动回滚本次配置文件变更

运行时配置项（面板设置 / `settings` 表 / 环境变量）：

- `openresty_enabled` / `OPENRESTY_ENABLED`
- `openresty_bin` / `OPENRESTY_BIN`
- `openresty_sites_dir` / `OPENRESTY_SITES_DIR`
- `openresty_conf_path` / `OPENRESTY_CONF_PATH`
- `php_fpm_addr` / `PHP_FPM_ADDR`
- `openresty_listen_ipv6` / `OPENRESTY_LISTEN_IPV6`

### OpenResty 快速安装与网站管理守卫

- 网站创建/编辑/启停/域名管理需先安装 OpenResty
- 快速安装支持 Linux 与 Windows
- 新增 API：
  - `GET /api/settings/openresty/status`
  - `POST /api/settings/openresty/install`
- Linux 快速安装会：
  - 使用发行版包管理器安装 OpenResty
  - 自动检测二进制与主配置路径
  - 自动选择/注入站点配置目录
  - 持久化设置并执行 `nginx -t`
- Windows 依次尝试：
  - `winget` → `choco` → `scoop`
- 若 Windows 包管理器不可用/失败，会降级为：
  - 从 `openresty.org` 下载最新官方 win64 压缩包
  - 自动解压并部署到 `C:\openresty`
- 前端通用日志弹窗组件：
  - `web/src/components/OperationLogDialog.vue`

## English

### One-Command Auto Deploy (Linux)

Use the command below to install dependencies, build, and deploy LinxPanel:

- `bash -c "$(curl -sSL https://raw.githubusercontent.com/wdcodeku/LinxPanel-Rust/main/scripts/install.sh)"`

Defaults:

- The first prompt asks language choice (English by default)
- The second prompt asks install mode (binary install by default)
- Install path: `/opt/linxpanel`
- Port: `7200`
- Auto-create and start `systemd` service (if available)
- If system `cargo` is too old, installer auto switches to `rustup stable`
- Installer prints a real access IP at the end (prefers public IP)
- Installs CLI tool automatically: `Lp` (alias: `lp`)
- For non-interactive deployment, set language explicitly:
  - `LINXPANEL_LANG=zh bash -c "$(curl -sSL https://raw.githubusercontent.com/wdcodeku/LinxPanel-Rust/main/scripts/install.sh)"`
  - Set install mode: `LINXPANEL_INSTALL_MODE=binary` or `LINXPANEL_INSTALL_MODE=build`
  - To force output IP manually: `LINXPANEL_PUBLIC_IP=1.2.3.4`

### Lp CLI Tool

- `Lp status`: show service status
- `Lp start|stop|restart`: manage service lifecycle
- `Lp logs [out|err|all|service]`: tail logs
- `Lp info`: show install path, port, and access URL
- `Lp update [binary|build]`: one-command upgrade (default: binary)

### Single Binary Deployment (Embedded Web)

LinxPanel embeds `web/dist` into the Rust executable at build time.  
After build, only one backend binary is required for deployment.

- Windows:
  - `powershell -ExecutionPolicy Bypass -File .\scripts\build-single.ps1`
  - Output binary: `target\release\linxpanel.exe`
- Linux:
  - `bash ./scripts/build-single.sh`
  - Output binary: `target/release/linxpanel`
- Manual build flow:
  - `cd web && npm ci && npm run build`
  - `cd .. && cargo build --release`

Notes:

- If `web/dist` is missing, build still succeeds but embeds a placeholder page.
- For production, always run web build before `cargo build --release`.

### OpenResty Real-Time Apply

Site/domain changes are now applied to OpenResty automatically:

- Generate per-site config files in `sites-enabled`
- Run `nginx -t` before reload
- Reload OpenResty only after config validation passes
- Auto-rollback changed files if test/reload fails

Runtime keys (panel settings / `settings` table / env):

- `openresty_enabled` / `OPENRESTY_ENABLED`
- `openresty_bin` / `OPENRESTY_BIN`
- `openresty_sites_dir` / `OPENRESTY_SITES_DIR`
- `openresty_conf_path` / `OPENRESTY_CONF_PATH`
- `php_fpm_addr` / `PHP_FPM_ADDR`
- `openresty_listen_ipv6` / `OPENRESTY_LISTEN_IPV6`

### OpenResty Quick Install & Site Guard

- Site create/edit/start/stop/domain management requires OpenResty installed
- Quick install supports both Linux and Windows
- New APIs:
  - `GET /api/settings/openresty/status`
  - `POST /api/settings/openresty/install`
- Linux quick install will:
  - install OpenResty via distro package manager
  - auto-detect binary and main config path
  - auto-select/include a site config directory
  - persist runtime settings and run `nginx -t`
- Windows quick install tries package managers in order:
  - `winget` → `choco` → `scoop`
- If package managers are unavailable/failed on Windows, fallback to:
  - download latest official OpenResty win64 zip from `openresty.org`
  - extract and deploy to `C:\openresty`
- Reusable frontend operation log dialog component:
  - `web/src/components/OperationLogDialog.vue`
