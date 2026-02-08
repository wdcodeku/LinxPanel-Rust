# LinxPanel - Modern Linux Server Management Panel (Rust + Vue 3)

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
