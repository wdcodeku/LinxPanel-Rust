# LinxPanel - Modern Linux Server Management Panel (Rust + Vue 3)

## Single Binary Deployment (Embedded Web)

LinxPanel now embeds `web/dist` into the Rust executable at build time.  
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

## OpenResty Real-Time Apply

Site/domain changes are now applied to OpenResty automatically:

- Generate per-site config files in `sites-enabled`.
- Run `nginx -t` before reload.
- Reload OpenResty only after config validation passes.
- Auto-rollback the changed file if test/reload fails.

Runtime keys (in panel settings / `settings` table / env):

- `openresty_enabled` / `OPENRESTY_ENABLED`
- `openresty_bin` / `OPENRESTY_BIN`
- `openresty_sites_dir` / `OPENRESTY_SITES_DIR`
- `openresty_conf_path` / `OPENRESTY_CONF_PATH`
- `php_fpm_addr` / `PHP_FPM_ADDR`
- `openresty_listen_ipv6` / `OPENRESTY_LISTEN_IPV6`

## OpenResty Quick Install & Site Guard

- Site create/edit/start/stop/domain management now requires OpenResty to be installed.
- Quick install supports both Linux and Windows.
- New APIs:
  - `GET /api/settings/openresty/status`
  - `POST /api/settings/openresty/install`
- Quick install (Linux) will:
  - install OpenResty by distro package manager,
  - auto-detect binary and main config path,
  - auto-select/include a site config directory,
  - persist runtime settings and run `nginx -t`.
- Quick install (Windows) tries package managers in order:
  - `winget` → `choco` → `scoop`
- If package managers are unavailable/failed on Windows, panel will fallback to:
  - download latest official OpenResty win64 zip from `openresty.org`
  - extract and deploy to `C:\openresty`
- Frontend provides a reusable operation log dialog component:
  - `web/src/components/OperationLogDialog.vue`
