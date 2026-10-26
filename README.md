# LinxPanel - Modern Linux Server Management Panel (Rust + Vue 3)

## 中文版

### Linux 一键自动部署

```bash
bash -c "$(curl -fsSL https://raw.githubusercontent.com/wdcodeku/LinxPanel-Rust/main/scripts/install.sh)"
```

默认行为：

- 首个提示选择语言（默认英文），第二个提示选择安装方式（默认二进制安装）
- **二进制安装**：下载 GitHub Release 中的静态编译包（约 6MB，musl 静态链接，任意发行版 / glibc 版本均可运行，支持 x86_64、ARM64、ARMv7、x86 32 位、RISC-V 64、龙芯 LoongArch64、ppc64le、s390x，按 `uname -m` 自动选择），并做 SHA256 校验，通常十几秒完成；无可用发布包时自动回退为编译安装
- **编译安装**：自动安装构建依赖；系统 Node.js 低于 18 时自动安装官方 Node.js，`cargo` 过旧时自动安装 `rustup stable`；内存小于 2GB 时单线程编译以避免 OOM
- 安装目录：`/opt/linxpanel`，监听端口：`7200`（安装前检测端口占用）
- **首次安装会生成随机管理员密码**并在结束时显示，不再使用公开的默认密码
- **首次安装默认启用随机安全入口和面板 HTTPS**（自签证书，HTTP 访问自动跳转 HTTPS），未带安全入口的访问一律返回 404
- 自动在已启用的 ufw / firewalld 中放行面板端口，配置 logrotate 日志轮转
- 自动创建并启动 `systemd` 服务（无 systemd 时使用 nohup）
- 启动后进行健康检查，确认面板可访问才显示「部署完成」
- 自动安装命令行工具：`Lp`（别名：`lp`）

重复执行同一命令即为**升级**：保留端口和数据，升级前备份二进制和数据库到 `/opt/linxpanel/backup/`（保留最近 5 份），新版本启动失败会**自动回滚**。

可选环境变量：

| 变量 | 说明 |
| --- | --- |
| `LINXPANEL_LANG=zh` | 语言（无交互部署时使用） |
| `LINXPANEL_INSTALL_MODE=binary\|build` | 安装方式 |
| `LINXPANEL_PORT=8888` | 面板端口 |
| `LINXPANEL_VERSION=v0.1.0` | 安装指定版本（默认最新） |
| `LINXPANEL_ADMIN_PASSWORD=...` | 指定管理员密码（默认随机生成） |
| `LINXPANEL_ENTRANCE=random\|off\|/自定义路径` | 安全入口（首次安装默认 `random`） |
| `LINXPANEL_SSL=1\|0` | 面板 HTTPS（首次安装默认 `1`，自签证书） |
| `LINXPANEL_GITHUB_PROXY=https://ghfast.top/` | GitHub 加速代理前缀（国内服务器下载慢时使用；代理可看到并提供下载内容，请仅使用可信代理） |
| `LINXPANEL_RELEASE_BASE_URL=https://dl.example.com/linxpanel/v0.1.0` | 从自建镜像 / CDN 下载发布包（目录内放 `linxpanel-linux-<架构>.tar.gz` 与 `.sha256`，即 Release 附件），不经过 GitHub |
| `LINXPANEL_NODE_MIRROR=https://npmmirror.com/mirrors/node` | 编译安装时的 Node.js 下载镜像 |
| `LINXPANEL_OPEN_FIREWALL=0` | 不自动放行防火墙端口 |
| `LINXPANEL_PUBLIC_IP=1.2.3.4` | 手动指定输出的访问 IP |

示例：`LINXPANEL_LANG=zh LINXPANEL_PORT=8888 bash -c "$(curl -fsSL https://raw.githubusercontent.com/wdcodeku/LinxPanel-Rust/main/scripts/install.sh)"`

### Lp 命令行工具

- `Lp status`：查看服务状态
- `Lp start|stop|restart`：启停/重启服务
- `Lp logs [out|err|all|service]`：查看日志
- `Lp info`：查看版本、安装路径、端口与访问地址
- `Lp update [binary|build]`：一键升级（默认二进制，失败自动回滚）
- `Lp reset-password [用户名]`：重置密码（生成新的随机密码，并让该用户所有已登录会话失效）
- `Lp entrance [show|off|random|/路径]`：查看或修改安全入口（修改后自动重启面板）
- `Lp ssl [status|on|off]`：查看或开关面板 HTTPS（修改后自动重启面板）
- `Lp port [端口|default]`、`Lp allow-ip [off|IP ...]`、`Lp domain [off|域名]`：修改面板端口、授权 IP、绑定域名（自动重启面板）
- `Lp mfa [off]`：查看或关闭两步验证
- `Lp security`：查看全部面板安全设置
- `Lp bans`、`Lp ban IP [分钟]`、`Lp unban IP|all`：查看 / 封禁 / 解封地址（立即生效；误封自己时用 `Lp unban all`）
- `Lp defense [on|off]`：查看或开关自动封禁；`Lp harden [on|off]`：内核网络加固
- `Lp version`：查看版本
- `Lp uninstall [--purge]`：卸载（默认保留数据，`--purge` 同时删除数据）

### 发布预编译包（维护者）

推送 `v*` 标签（如 `git tag v0.1.0 && git push origin v0.1.0`）会触发 `.github/workflows/release.yml`，自动构建全部 8 种架构的静态二进制（每种都在 qemu 中实际启动并做 HTTP 冒烟测试后才发布），附带 SHA256SUMS，发布到 GitHub Release，一键安装脚本会直接下载它。**在发布第一个 Release 之前，一键安装只能走编译安装（较慢）。**

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

### 实时监控内核

面板内置单一采样引擎，后台每秒采样一次，所有连接的客户端共享同一份数据：

- `GET /api/realtime/ws`：WebSocket 推送，连接即回填最近 10 分钟历史，随后每秒推送一帧
- `GET /api/realtime/snapshot`：REST 快照（静态信息 + 历史 + 最新采样 + 磁盘/网卡明细）
- 采样内容：CPU 总体与每核、内存/Swap、负载、网络上下行速率、磁盘读写速率
- 速率在服务端按真实时间差计算，前端不再自行推算
- 磁盘枚举与网卡明细每 5 秒刷新一次，CPU/内存每秒刷新，避免高频枚举开销
- WebSocket 通过 `?token=` 携带 JWT；断线自动指数退避重连

### 进程管理

- `GET /api/processes`：支持关键字搜索、排序（cpu/memory/pid/name/run_time）、分页
- `POST /api/processes/{pid}/signal`：发送 TERM / INT / HUP / KILL / STOP / CONT
- 进程 CPU 归一化为「占整机百分比」，启动时预热基线，首次打开即有真实数值

### 系统服务（Linux / systemd）

- `GET /api/services`、`GET /api/services/backend`
- `POST /api/services/action`：start / stop / restart / reload / enable / disable
- `GET /api/services/{name}/status`、`GET /api/services/{name}/logs`（journalctl）
- 无 root 或免密 sudo 时自动降级为只读；macOS 以 launchd 只读方式列出

### 计划任务

- `GET/POST /api/cron`、`POST /api/cron/{id}`、`DELETE /api/cron/{id}`、`POST /api/cron/{id}/toggle`
- 通过 `crontab -` 原子写回，任务名以 `#LinxPanel:` 注释保存
- 停用任务以注释形式保留，不会丢失原始内容

### 防火墙与端口

- `GET /api/firewall/status`、`GET/POST /api/firewall/rules`、`DELETE /api/firewall/rules/{id}`
- `POST /api/firewall/toggle`：启停防火墙
- 支持 ufw 与 firewalld，端口与来源地址均做白名单校验
- `GET /api/network/ports`：列出监听端口及占用进程（ss → lsof → netstat 自动降级）
- 只读探测在无 sudo 时自动降级为非特权执行，不会直接报错

### 面板安全设置（类似宝塔 / 1Panel）

「面板设置 → 安全」以列表形式集中管理以下设置，点击「设置」在弹窗中修改：

| 设置 | 说明 | 服务器端恢复命令 |
| --- | --- | --- |
| 面板端口 | 修改后重启生效，自动在 ufw/firewalld 放行新端口；新端口被占用时启动会自动退回原端口 | `Lp port 7200` |
| 安全入口 | 必须先访问 `/入口路径` 才能打开面板，签名 Cookie 记住 30 天 | `Lp entrance` / `Lp entrance off` |
| 面板 SSL | 自签证书或证书库中的证书，HTTP 自动 301 跳转 HTTPS；证书损坏时自动退回 HTTP | `Lp ssl off` |
| 授权 IP | 仅允许指定 IP / CIDR 网段（IPv4、IPv6）访问；列表必须包含当前 IP，防止误锁 | `Lp allow-ip off` |
| 绑定域名 | 只允许通过该域名访问；须先通过该域名打开面板才能绑定，防止误锁 | `Lp domain off` |
| 未认证响应 | 被拒绝时返回提示页（默认）或普通 404 / 403 / 400 页面，不暴露面板 | — |
| 面板用户 | 修改登录用户名（需当前密码） | — |
| 面板密码 | 修改后该用户其它所有会话立即失效 | `Lp reset-password` |
| 两步验证 | TOTP 动态验证码（Google Authenticator / Microsoft Authenticator / 1Password 等），扫码绑定，验证码防重放 | `Lp mfa off` |
| 会话超时 | 登录有效期 | — |

此外：同一 IP 15 分钟内登录失败 5 次即锁定（两步验证码错误同样计数）；`Lp security` 可在服务器上查看全部设置；systemd 下可直接在页面重启面板。

### 主动防御（抗攻击）

面向“被大量攻击导致服务器瘫痪”的场景，原则是**越早拒绝越便宜**：恶意流量在内核层直接丢弃，不进入 nginx、PHP 或面板。不依赖 Docker，没有常驻的额外进程。在「安全防护 → 主动防御」中设置。

| 层级 | 作用 |
| --- | --- |
| 内核封禁（nftables） | 被封地址的数据包在内核 input 钩子直接丢弃（无 TCP 握手、无 TLS、无应用开销），按时长自动过期；与 ufw / firewalld 互不干扰；重启或 `nft flush` 后自动恢复 |
| 自动封禁引擎 | 每 5 秒增量读取各网站访问日志、SSH 认证日志（auth.log / secure / journald）和面板拒绝记录，按滑动窗口统计；超过阈值即封禁。内存有上限，日志每次最多读 8MB，攻击期间也不会拖垮服务器 |
| 网站 CC / 扫描 | 默认 60 秒内 600 次请求或 120 次 4xx（扫描 /wp-login.php、/.env 等）封禁 1 小时 |
| nginx 限流（可选） | 所有网站按 IP 限速（默认 30 次/秒，突发 200）和并发连接（64），超限返回 429 并计入封禁 |
| nginx 防慢速攻击 | 全局设置请求头 / 请求体 / 发送超时、超时连接立即释放、隐藏版本号（已在 nginx.conf 设置的项自动跳过，`nginx -t` 失败自动降级并回滚） |
| SSH 防爆破 | 默认 10 分钟内 6 次失败封禁；成功登录过的地址 24 小时内不会被封 |
| 面板自保护 | 每 IP 连接数上限（48）与总连接上限（1024）、请求头 10 秒超时（防 slowloris）、TLS 握手超时、密码校验并发上限（防 bcrypt 耗尽 CPU）、静态资源预压缩缓存；扫描面板端口的地址自动封禁；systemd 中提高面板优先级并降低被 OOM 杀死的概率 |
| 内核网络加固 | `/etc/sysctl.d/90-linxpanel.conf`：SYN Cookie、更大的半连接 / 全连接队列、缩短 FIN 超时、宽松反向路径过滤、禁止 ICMP 重定向与源路由（安装时默认应用，`LINXPANEL_HARDEN=0` 跳过） |

防误封：本机回环与服务器自身地址永不封禁；白名单支持 IP / CIDR；登录过面板或 SSH 的地址 24 小时内豁免；手动封禁时拒绝封禁当前访问者自己的 IP。被封后可在服务器控制台执行 `Lp unban all` 恢复。

注意：网站使用 CDN 时访问日志里是 CDN 节点 IP，请把 CDN 的 IP 段加入白名单（或在网站配置中启用 real_ip）。每个网站的访问日志位于 `安装目录/logs/web/site-<ID>.log`，由 logrotate 按天 / 按大小轮转。

### 会话持久化

JWT 签名密钥持久化在 `settings` 表中（`JWT_SECRET` 环境变量优先），**面板重启不再导致所有用户掉线**。

### 运行环境与应用托管

面板内置语言运行环境检测与安装，并托管应用进程：

- `GET /api/runtimes`：检测 PHP / Node.js / Python / Java / Go 的版本、路径与配套工具（npm、pip、composer、maven…）
- `POST /api/runtimes/install`：通过发行版包管理器安装（apt / dnf / yum / apk，macOS 用 brew），返回完整安装日志
- 应用托管（`/api/apps`）支持 Node / Python / Java(jar) / Go / 二进制 / PHP-CLI：
  - 工作目录、启动文件、启动参数、环境变量、端口
  - 设置端口后自动注入 `PORT` 环境变量
  - 崩溃自动重启，指数退避（2s → 60s），进程存活 30s 后退避重置
  - stdout/stderr 落盘为日志，超过 8MB 自动截断
  - 面板重启后按 `autostart` 自动拉起
  - **面板收到 SIGTERM 时会先停掉所有托管进程**，不留孤儿进程占用端口

### 数据库、应用商店与备份

- **数据库**（侧边栏「数据库」）：MySQL（MariaDB）、PostgreSQL、Redis 一键安装、启动并开机自启
  - 新建数据库同时创建同名用户与随机密码，可选访问权限（本机 / 所有 / 指定 IP）；密码可查看、复制、重置
  - 安装 MySQL 时自动创建面板专用管理账号；已有 MySQL 可输入 root 密码接管
  - 备份为 `.sql.gz`（mysqldump / pg_dump），支持下载、一键恢复、删除
  - Redis 可设置访问密码
  - 所有库名、用户名、密码均经白名单校验，SQL 与密码通过 stdin 传给客户端，不会出现在进程列表中
- **应用商店**：一键部署 WordPress、Typecho、Adminer
  - 自动检查 OpenResty / PHP / MySQL，自动建库、下载解压、写入配置（WordPress 自动生成 `wp-config.php` 与安全密钥）、设置目录属主并创建站点
  - 任何一步失败都会回滚已创建的数据库、文件和站点
  - 可填写自定义下载地址（国内镜像）
- **PHP 网站开箱即用**：安装 PHP 时自动安装常用扩展（mysql、gd、curl、mbstring、zip、intl 等）、启动 PHP-FPM，并自动识别其 socket 地址、放开 socket 权限（此前固定 `127.0.0.1:9000`，在 Debian/Ubuntu/RHEL 上 PHP 站点会 502）；已安装的 PHP 可在「运行环境」中一键「检查修复」
- **站点默认加固**：上传上限 256MB、禁止访问 `.env` / `.git` 等隐藏文件、FastCGI 超时 300 秒；服务器未启用 IPv6 时不再生成 `listen [::]`（否则 nginx 无法启动）
- **项目依赖**（运行环境 → 应用 →「安装依赖」）：Python 自动创建 `.venv` 并 `pip install -r requirements.txt`；Node 按锁文件使用 npm / pnpm / yarn 安装并执行 build；PHP 执行 `composer install`。可在应用环境变量中设置 `PIP_INDEX_URL`、`npm_config_registry` 使用镜像
  - 启动入口支持 `main.py`、`-m uvicorn main:app`、`gunicorn app:app`、`npm start`、`next start` 等写法，自动使用项目的 `.venv` 与 `node_modules/.bin`
- **备份**：站点详情「备份」页可立即备份整站目录（`.tar.gz`）、下载、恢复（恢复失败自动还原原目录）；计划任务新增「备份网站 / 备份数据库」类型，可设置保留份数（底层为 `linxpanel backup site|db ... --keep N`）

### 网站类型与伪静态

- 站点类型：静态 / PHP / Node / Python / Java / Go / 二进制 / 反向代理
- 应用类站点绑定一个托管应用，自动生成反向代理配置（含 WebSocket 升级头透传）
- 伪静态规则支持预设（WordPress、Laravel/ThinkPHP、强制 HTTPS）
- 规则经白名单校验：仅允许 `rewrite` / `return` / `set` / `if` / `break`，拒绝花括号等可能逃逸 `server` 块的写法（含单元测试）

### 终端

- 独立终端页支持多标签，标签保持各自的会话与滚动历史
- 终端封装为可复用组件 `web/src/components/TerminalPane.vue`，可内嵌到任意页面
- 目录跳转、清屏、断开/重连，断线自动指数退避重连

### 面板设置

分为「基础 / 安全 / 告警 / 关于」四个标签页：

- 基础：面板名称、端口、API 地址、OpenResty 路径与 PHP-FPM 地址
- 安全：会话有效期、修改密码、安全自检（默认账号、HTTPS、会话时长、网络接口）
- 告警：CPU / 内存 / 磁盘 阈值
- 关于：面板版本与实时系统信息

**`session_timeout` 现在真正生效**（此前存入数据库但被硬编码的 24 小时忽略）。

### SSL 证书管理

证书存放在 `data/certs/<id>/`（私钥 `0600`，证书 `0644`），元数据由 `openssl` 解析，不引入额外的 X.509 依赖。

- `GET /api/certs`：列出证书，含域名(SAN)、颁发机构、有效期、剩余天数、状态与占用站点
- `POST /api/certs/upload`：粘贴 PEM 证书链与私钥
- `POST /api/certs/self-signed`：生成自签名证书（内网/测试用）
- `POST /api/certs/acme`：Let's Encrypt 申请（HTTP-01 webroot 验证），支持 staging 试跑
- `POST /api/certs/{id}/renew`：手动续期
- `GET /api/certs/acme/status`：检测 acme.sh / certbot
- `POST /api/sites/{id}/ssl`：绑定/解绑证书到站点

安全与正确性：

- **上传时校验私钥与证书是否匹配**，不匹配直接拒绝，避免 nginx 重载失败导致站点全挂
- 域名与邮箱经严格白名单校验后才作为参数传给 openssl / ACME 客户端
- 证书被站点占用时拒绝删除
- **自动续期**：后台任务每天检查一次，剩余不足 30 天的 ACME 证书自动续期，并重载使用它的站点

生成的 TLS 配置：

- `TLSv1.2 TLSv1.3`，仅前向保密套件，`ssl_session_tickets off`
- 可选强制 HTTPS 跳转（80 端口跳转 server，**保留 `.well-known/acme-challenge/` 不跳转**，否则续期会失败）
- 可选 HSTS（一年、含子域名）

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

### 生产环境就绪度

已具备：静态二进制一键部署与升级回滚、面板安全（安全入口 / SSL / 授权 IP / 绑定域名 / 两步验证 / 登录锁定）、主动防御（内核封禁 / 自动封禁 / 防 CC / 防爆破 / 内核加固）、网站与 SSL 自动续期、PHP / Node / Python / Java / Go 运行环境与进程守护、MySQL / PostgreSQL / Redis 管理、WordPress 等一键部署、网站与数据库定时备份、防火墙、计划任务、文件管理、终端。

与宝塔 / 1Panel 相比仍缺少（后续计划）：

- Docker 与容器应用商店（1Panel 的核心能力）
- 多 PHP 版本共存（目前使用发行版默认版本）、MySQL 版本选择
- 资源告警通知（阈值设置已有，尚未接入邮件 / Webhook）、操作审计日志
- 备份上传到远程存储（S3 / OSS / FTP）
- 网站访问日志分析、规则型 WAF（SQL 注入 / XSS 特征拦截）
- 多用户与权限管理

## English

### One-Command Auto Deploy (Linux)

```bash
bash -c "$(curl -fsSL https://raw.githubusercontent.com/wdcodeku/LinxPanel-Rust/main/scripts/install.sh)"
```

Defaults:

- The first prompt asks for a language (English by default), the second for the install mode (binary by default)
- **Binary install** downloads the static release package from GitHub Releases (~6MB, musl static, runs on any distro / glibc; x86_64, ARM64, ARMv7, 32-bit x86, RISC-V 64, LoongArch64, ppc64le and s390x, picked by `uname -m`) and verifies its SHA256 — usually done in seconds. Falls back to a source build when no release asset is available
- **Source build** installs build dependencies, installs official Node.js when the system one is older than 18, switches to `rustup stable` when `cargo` is too old, and compiles single-threaded under 2GB RAM to avoid OOM
- Install path `/opt/linxpanel`, port `7200` (checked for conflicts before installing)
- **A fresh install generates a random admin password** and prints it at the end instead of using a published default
- **A fresh install enables a random security entrance and panel HTTPS** (self-signed, HTTP redirects to HTTPS); requests without the entrance get a 404
- Opens the panel port in an active ufw / firewalld, and sets up logrotate
- Creates and starts a `systemd` service (nohup when systemd is unavailable)
- Health-checks the panel and only reports success once it answers
- Installs the CLI tool `Lp` (alias `lp`)

Running the same command again **upgrades** in place: the port and data are kept, the binary and database are backed up to `/opt/linxpanel/backup/` (last 5 kept), and a new version that fails to start is **rolled back automatically**.

Optional environment variables:

| Variable | Meaning |
| --- | --- |
| `LINXPANEL_LANG=zh` | Language (for non-interactive runs) |
| `LINXPANEL_INSTALL_MODE=binary\|build` | Install mode |
| `LINXPANEL_PORT=8888` | Panel port |
| `LINXPANEL_VERSION=v0.1.0` | Install a specific release (default: latest) |
| `LINXPANEL_ADMIN_PASSWORD=...` | Set the admin password (default: random) |
| `LINXPANEL_ENTRANCE=random\|off\|/custom-path` | Security entrance (fresh install default: `random`) |
| `LINXPANEL_SSL=1\|0` | Panel HTTPS (fresh install default: `1`, self-signed) |
| `LINXPANEL_GITHUB_PROXY=https://ghfast.top/` | Prefix for github.com downloads where GitHub is slow (the proxy sees and serves the files, so only use one you trust) |
| `LINXPANEL_RELEASE_BASE_URL=https://dl.example.com/linxpanel/v0.1.0` | Download release packages from your own mirror / CDN (a directory with the release's `linxpanel-linux-<arch>.tar.gz` and `.sha256` files) instead of GitHub |
| `LINXPANEL_NODE_MIRROR=https://npmmirror.com/mirrors/node` | Node.js download mirror for source builds |
| `LINXPANEL_OPEN_FIREWALL=0` | Do not open the firewall port |
| `LINXPANEL_PUBLIC_IP=1.2.3.4` | Override the printed access IP |

### Lp CLI Tool

- `Lp status`: show service status
- `Lp start|stop|restart`: manage service lifecycle
- `Lp logs [out|err|all|service]`: tail logs
- `Lp info`: show version, install path, port, and access URL
- `Lp update [binary|build]`: one-command upgrade (default: binary, rolls back on failure)
- `Lp reset-password [user]`: set a new random password and sign out that user's sessions
- `Lp entrance [show|off|random|/path]`: show or change the security entrance (restarts the panel)
- `Lp ssl [status|on|off]`: show or toggle panel HTTPS (restarts the panel)
- `Lp port [PORT|default]`, `Lp allow-ip [off|IP ...]`, `Lp domain [off|DOMAIN]`: change the panel port, allowed IPs, bound domain (restarts the panel)
- `Lp mfa [off]`: show or turn off two-factor login
- `Lp security`: show every panel security setting
- `Lp bans`, `Lp ban IP [MINUTES]`, `Lp unban IP|all`: list / ban / lift bans (immediate; `Lp unban all` if you locked yourself out)
- `Lp defense [on|off]`: show or switch automatic banning; `Lp harden [on|off]`: kernel network hardening
- `Lp version`: show the installed version
- `Lp uninstall [--purge]`: remove LinxPanel (data is kept unless `--purge`)

### Publishing Prebuilt Binaries (maintainers)

Pushing a `v*` tag (e.g. `git tag v0.1.0 && git push origin v0.1.0`) runs `.github/workflows/release.yml`, which builds static binaries for all 8 architectures (each one is started under qemu and smoke-tested over HTTP before publishing), with SHA256SUMS, and attaches them to a GitHub Release that the installer downloads. **Until the first release exists, the installer can only build from source (slow).**

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

### Realtime Metrics Engine

A single background sampler ticks once per second and every connected client shares it:

- `GET /api/realtime/ws`: WebSocket push; backfills the last 10 minutes on connect, then one frame per second
- `GET /api/realtime/snapshot`: REST snapshot (static info + history + latest sample + disk/NIC detail)
- Sampled: global and per-core CPU, memory/swap, load average, network rx/tx rates, disk read/write rates
- Rates are computed server-side from real elapsed time; the client no longer derives them
- Disk enumeration and NIC detail refresh every 5s while CPU/memory refresh every 1s
- The WebSocket carries its JWT via `?token=`; the client reconnects with exponential backoff

### Process Management

- `GET /api/processes`: keyword search, sorting (cpu/memory/pid/name/run_time), pagination
- `POST /api/processes/{pid}/signal`: TERM / INT / HUP / KILL / STOP / CONT
- Process CPU is normalised to percent-of-machine, and the baseline is primed at startup

### System Services (Linux / systemd)

- `GET /api/services`, `GET /api/services/backend`
- `POST /api/services/action`: start / stop / restart / reload / enable / disable
- `GET /api/services/{name}/status`, `GET /api/services/{name}/logs` (journalctl)
- Degrades to read-only without root or passwordless sudo; macOS lists launchd read-only

### Cron Jobs

- `GET/POST /api/cron`, `POST /api/cron/{id}`, `DELETE /api/cron/{id}`, `POST /api/cron/{id}/toggle`
- Written back atomically through `crontab -`; job names persist as `#LinxPanel:` comments
- Disabled jobs stay in the crontab as comments, so nothing is lost

### Firewall & Ports

- `GET /api/firewall/status`, `GET/POST /api/firewall/rules`, `DELETE /api/firewall/rules/{id}`
- `POST /api/firewall/toggle` to enable/disable the firewall
- Supports ufw and firewalld; ports and source addresses are validated against an allowlist
- `GET /api/network/ports`: listening sockets with owning process (ss → lsof → netstat fallback)
- Read-only probes fall back to unprivileged execution instead of erroring when sudo is unavailable

### Panel Security Settings (like BaoTa / 1Panel)

Settings → Security lists every setting with its current value; "Set" edits it in a dialog:

| Setting | What it does | Recovery on the server |
| --- | --- | --- |
| Panel port | Applies after a restart and is opened in ufw/firewalld automatically; if the new port is taken at startup the panel falls back to the previous one | `Lp port 7200` |
| Security entrance | The panel only opens after visiting `/<entrance>`; a signed cookie remembers the browser for 30 days | `Lp entrance` / `Lp entrance off` |
| Panel SSL | Self-signed or any certificate from the store; HTTP gets a 301 to HTTPS; a broken certificate falls back to HTTP | `Lp ssl off` |
| Allowed IPs | Only these IPs / CIDR ranges (IPv4, IPv6) may connect; the list must include your current IP so you cannot lock yourself out | `Lp allow-ip off` |
| Bound domain | Only answer for this domain; you must open the panel through it before binding | `Lp domain off` |
| Unauthorized response | Rejected visitors get a hint page (default) or a plain 404 / 403 / 400 that does not reveal the panel | — |
| Panel user | Rename the login user (needs the current password) | — |
| Panel password | Changing it signs out every other session of the user | `Lp reset-password` |
| Two-factor login | TOTP codes (Google Authenticator, Microsoft Authenticator, 1Password, …) set up by QR code, with replay protection | `Lp mfa off` |
| Session timeout | How long a sign-in lasts | — |

Also: 5 failed logins from one IP within 15 minutes lock it out (wrong two-factor codes count too); `Lp security` prints every setting on the server; under systemd the panel can restart itself from the page.

### Active Defense (surviving attacks)

Built for servers that attacks used to bring down. The rule is **reject as early as possible**: abusive traffic is dropped in the kernel and never reaches nginx, PHP or the panel. No Docker and no extra resident processes. Configure it under Security → Active defense.

| Layer | What it does |
| --- | --- |
| Kernel bans (nftables) | Packets from banned addresses are dropped at the kernel input hook (no TCP handshake, no TLS, no application work) and expire automatically; independent of ufw / firewalld; restored after a reboot or `nft flush` |
| Auto-ban engine | Every 5 s it reads what was appended to each site's access log, the SSH auth log (auth.log / secure / journald) and the panel's rejections, counting in sliding windows; crossing a threshold bans the address. Memory is capped and at most 8 MB of log is read per tick, so it stays cheap during an attack |
| Website CC / scanners | Default: 600 requests or 120 4xx responses (probing /wp-login.php, /.env ...) within 60 s bans for an hour |
| nginx rate limit (optional) | Per-IP request rate (default 30/s, burst 200) and connection cap (64) on every site; excess gets 429 and counts toward a ban |
| nginx anti-slowloris | Global header / body / send timeouts, timed-out connections reset at once, version hidden (directives already in nginx.conf are skipped; a failing `nginx -t` degrades and rolls back) |
| SSH brute force | Default: 6 failures within 10 minutes bans; addresses that logged in successfully are exempt for 24 hours |
| Panel self-protection | Per-IP (48) and total (1024) connection caps, 10 s header timeout (slowloris), TLS handshake timeout, bounded concurrent password checks (bcrypt cannot exhaust the CPU), pre-compressed cached assets; addresses scanning the panel port are banned; under systemd the panel runs at a higher priority and is a late OOM victim |
| Kernel network hardening | `/etc/sysctl.d/90-linxpanel.conf`: SYN cookies, larger SYN and accept backlogs, shorter FIN timeout, loose reverse-path filtering, no ICMP redirects or source routing (applied by the installer; `LINXPANEL_HARDEN=0` skips it) |

Lockout safety: loopback and the server's own addresses are never banned; the whitelist takes IPs and CIDRs; addresses that logged in to the panel or SSH are exempt for 24 hours; a manual ban of your own current IP is refused. If you do get banned, run `Lp unban all` from the server console.

Behind a CDN the access logs show CDN addresses: whitelist the CDN's ranges (or enable real_ip for the site). Each site logs to `<install dir>/logs/web/site-<ID>.log`, rotated daily / by size by logrotate.

### Session Persistence

The JWT signing secret is persisted in the `settings` table (`JWT_SECRET` env wins), so **restarting the panel no longer logs everyone out**.

### Runtimes and Managed Applications

The panel detects and installs language runtimes, and supervises application processes:

- `GET /api/runtimes`: detects PHP / Node.js / Python / Java / Go with versions, paths and companion tooling (npm, pip, composer, maven…)
- `POST /api/runtimes/install`: installs through the distro package manager (apt / dnf / yum / apk, brew on macOS) and returns the full install log
- Managed applications (`/api/apps`) support Node / Python / Java (jar) / Go / binary / PHP-CLI:
  - working directory, entry point, arguments, environment variables, port
  - a `PORT` environment variable is injected when a port is set
  - crash auto-restart with exponential backoff (2s → 60s), reset once a process stays up 30s
  - stdout/stderr captured to a log file, truncated past 8MB
  - relaunched after a panel restart when `autostart` is set
  - **SIGTERM stops every managed process first**, so no orphans keep holding ports

### Databases, App Store and Backups

- **Databases** (sidebar → Databases): one-click MySQL (MariaDB), PostgreSQL and Redis - installed, started and enabled at boot
  - New databases get a same-named user with a random password and an access scope (localhost / anywhere / IP); passwords can be shown, copied and reset
  - Installing MySQL creates a panel-owned admin login; an existing MySQL can be connected with its root password
  - Backups as `.sql.gz` (mysqldump / pg_dump) with download, one-click restore and delete
  - Redis password setting
  - Names, users and passwords are allowlisted; SQL and passwords reach the clients on stdin, never in the process list
- **App Store**: one-click WordPress, Typecho and Adminer
  - Checks OpenResty / PHP / MySQL, creates the database, downloads and unpacks, writes the config (`wp-config.php` with fresh keys for WordPress), sets ownership and creates the site
  - Any failure rolls back the database, files and site it created
  - A custom download URL (mirror) can be given
- **PHP sites work out of the box**: installing PHP adds common extensions (mysql, gd, curl, mbstring, zip, intl...), starts PHP-FPM, detects its socket and opens it to nginx (the fixed `127.0.0.1:9000` used before gave every PHP site a 502 on Debian/Ubuntu/RHEL); an installed PHP can be repaired with "Check & repair" under Runtimes
- **Hardened site defaults**: 256MB uploads, dotfiles (`.env`, `.git`) denied, 300s FastCGI timeout; no `listen [::]` on hosts without IPv6 (nginx would refuse to start)
- **Project dependencies** (Runtimes → Apps → "Install dependencies"): Python gets a `.venv` and `pip install -r requirements.txt`; Node uses npm / pnpm / yarn per lockfile and runs the build script; PHP runs `composer install`. Set `PIP_INDEX_URL` / `npm_config_registry` in the app's environment for mirrors
  - Entries such as `main.py`, `-m uvicorn main:app`, `gunicorn app:app`, `npm start` or `next start` work, using the project's `.venv` and `node_modules/.bin`
- **Backups**: the site's Backups tab archives the whole directory (`.tar.gz`), downloads and restores it (a failed restore puts the old files back); cron gains "Back up website / database" job types with retention (`linxpanel backup site|db ... --keep N` underneath)

### Site Types and Rewrite Rules

- Site types: static / PHP / Node / Python / Java / Go / binary / reverse proxy
- App-backed sites link to a managed application and generate the reverse proxy config, including WebSocket upgrade headers
- Rewrite ("pseudo-static") presets for WordPress, Laravel/ThinkPHP and force-HTTPS
- Rules are allowlisted: only `rewrite` / `return` / `set` / `if` / `break`, rejecting braces and anything else that could escape the generated `server` block (unit tested)

### Terminal

- A standalone terminal page with multiple tabs, each keeping its own session and scrollback
- The terminal is a reusable component, `web/src/components/TerminalPane.vue`, embeddable anywhere
- Directory jump, clear, disconnect/reconnect, and automatic exponential-backoff reconnect

### Panel Settings

Split into General / Security / Alerts / About:

- General: panel name, port, API address, OpenResty paths and PHP-FPM address
- Security: session lifetime, password change, and a security checklist (default account, HTTPS, session length, interfaces)
- Alerts: CPU / memory / disk thresholds
- About: panel version and live system information

**`session_timeout` is now actually applied** — it was previously stored but ignored in favour of a hardcoded 24 hours.

### SSL Certificate Management

Certificates live in `data/certs/<id>/` (key `0600`, cert `0644`), and their metadata is read back with `openssl` rather than adding an X.509 parsing dependency.

- `GET /api/certs`: lists certificates with SAN domains, issuer, validity, days remaining, status and the sites using them
- `POST /api/certs/upload`: paste a PEM chain and private key
- `POST /api/certs/self-signed`: generate a self-signed certificate for internal/test use
- `POST /api/certs/acme`: issue via Let's Encrypt (HTTP-01 webroot), with a staging dry-run option
- `POST /api/certs/{id}/renew`: manual renewal
- `GET /api/certs/acme/status`: detects acme.sh / certbot
- `POST /api/sites/{id}/ssl`: bind or unbind a certificate on a site

Safety and correctness:

- **Uploads verify that the key matches the certificate** and are rejected otherwise, so a bad pair cannot break the next nginx reload
- Domains and emails are allowlist-validated before reaching openssl or the ACME client
- A certificate still bound to a site cannot be deleted
- **Auto-renewal**: a daily background sweep renews ACME certificates with under 30 days left and reloads the sites using them

Generated TLS config:

- `TLSv1.2 TLSv1.3`, forward-secrecy ciphers only, `ssl_session_tickets off`
- Optional forced HTTPS redirect (a port-80 redirect server that **keeps `.well-known/acme-challenge/` un-redirected**, or renewals would fail)
- Optional HSTS (one year, includeSubDomains)

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

### Production Readiness

In place: static-binary install with upgrade rollback; panel security (entrance, SSL, allowed IPs, bound domain, two-factor login, login lockout); active defense (kernel bans, auto-ban, CC limits, brute-force protection, kernel hardening); sites with auto-renewing SSL; PHP / Node / Python / Java / Go runtimes with process supervision; MySQL / PostgreSQL / Redis management; one-click WordPress and friends; scheduled website and database backups; firewall, cron, file manager and terminal.

Still missing compared with BaoTa / 1Panel (planned):

- Docker and a container app store (1Panel's core)
- Multiple PHP versions side by side (the distro's version is used), MySQL version choice
- Alert notifications (thresholds exist but are not wired to email / webhooks), an audit log
- Off-site backup targets (S3 / OSS / FTP)
- Access-log analytics, a signature WAF (SQL injection / XSS rules)
- Multiple users and roles
