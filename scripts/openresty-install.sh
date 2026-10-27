#!/usr/bin/env bash
# Installs OpenResty for LinxPanel: the official package repository first,
# a source build when the distro has no package. Run by the panel as root (or
# with USE_SUDO=1 through passwordless sudo); every step logs what it does so
# the panel's task window shows exactly where an install stops.
set -u

say() { echo "[LinxPanel] $*"; }

# Unattended package operations: no debconf / needrestart / conffile prompts
# (nothing can answer them from the panel), and wait for the dpkg lock that
# automatic updates hold for minutes after boot instead of failing at once.
# Passed through env because sudo drops the caller's environment.
run() {
  if [ "$1" = "apt-get" ]; then
    shift
    set -- apt-get -o DPkg::Lock::Timeout=600 \
      -o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold "$@"
  fi
  if [ "${USE_SUDO:-0}" = "1" ]; then
    sudo -n env DEBIAN_FRONTEND=noninteractive NEEDRESTART_MODE=a NEEDRESTART_SUSPEND=1 "$@"
  else
    env DEBIAN_FRONTEND=noninteractive NEEDRESTART_MODE=a NEEDRESTART_SUSPEND=1 "$@"
  fi
}

write_root_file() { # path, content on stdin
  if [ "${USE_SUDO:-0}" = "1" ]; then sudo -n tee "$1" >/dev/null; else cat >"$1"; fi
}

PREFIX="${OPENRESTY_PREFIX:-/usr/local/openresty}"
NGINX_BIN="${PREFIX}/nginx/sbin/nginx"
APT_LIST=/etc/apt/sources.list.d/openresty.list
KEYRING=/usr/share/keyrings/openresty.gpg

# ---------- apt helpers ----------

# A broken unrelated source (a leftover Docker repo, an unreachable mirror)
# makes `apt-get update` exit non-zero although every other index refreshed;
# that must not stop the install, so success is judged by package candidates.
apt_refresh() {
  if ! run apt-get update; then
    say "apt-get update reported errors (often an unrelated broken source); continuing"
  fi
}

apt_has() { # package has an install candidate
  local candidate
  candidate="$(apt-cache policy "$1" 2>/dev/null | awk '/Candidate:/ {print $2; exit}')"
  [ -n "$candidate" ] && [ "$candidate" != "(none)" ]
}

drop_openresty_repo() {
  # A repository that does not work would break every later `apt-get update`.
  run rm -f "$APT_LIST" "$KEYRING" 2>/dev/null || true
}

# ---------- repository install ----------

install_from_apt_repo() {
  local id="" like="" codename="" base candidates c tmp_key
  if [ -r /etc/os-release ]; then
    # shellcheck disable=SC1091
    . /etc/os-release
    id="${ID:-}"; like="${ID_LIKE:-}"; codename="${VERSION_CODENAME:-}"
  fi
  case "$id" in
    ubuntu) base="https://openresty.org/package/ubuntu"; candidates="$codename noble jammy focal" ;;
    debian) base="https://openresty.org/package/debian"; candidates="$codename trixie bookworm bullseye" ;;
    *)
      if echo "$like" | grep -qi ubuntu; then
        base="https://openresty.org/package/ubuntu"; candidates="${UBUNTU_CODENAME:-} noble jammy"
      elif echo "$like" | grep -qi debian; then
        base="https://openresty.org/package/debian"; candidates="bookworm bullseye"
      else
        say "No OpenResty package repository for ${id:-this system}"
        return 1
      fi
      ;;
  esac

  say "System: ${PRETTY_NAME:-$id $codename}"
  # A source left by an earlier failed attempt breaks every apt refresh.
  drop_openresty_repo
  run apt-get install -y curl gnupg ca-certificates || { apt_refresh; run apt-get install -y curl gnupg ca-certificates; }

  tmp_key="$(mktemp)"
  if ! curl -fsSL --retry 3 --connect-timeout 15 https://openresty.org/package/pubkey.gpg -o "$tmp_key"; then
    say "Cannot download the OpenResty signing key from openresty.org"
    rm -f "$tmp_key"
    drop_openresty_repo
    return 1
  fi
  run mkdir -p /usr/share/keyrings
  # --batch --yes: a keyring left by an earlier attempt is overwritten without
  # gpg asking on /dev/tty, which a panel install does not have.
  if ! run gpg --batch --yes --dearmor -o "$KEYRING" "$tmp_key"; then
    rm -f "$tmp_key"
    drop_openresty_repo
    return 1
  fi
  rm -f "$tmp_key"

  local tried=""
  for c in $candidates; do
    [ -n "$c" ] || continue
    case " $tried " in *" $c "*) continue ;; esac
    tried="$tried $c"
    say "Trying the OpenResty repository for '$c'"
    echo "deb [signed-by=${KEYRING}] ${base} ${c} openresty" | write_root_file "$APT_LIST"
    apt_refresh
    if ! apt_has openresty; then
      say "No OpenResty package for '$c'"
      continue
    fi
    if run apt-get install -y openresty; then
      say "Installed OpenResty from the repository ('$c')"
      return 0
    fi
    say "Installing the '$c' package failed (dependencies do not match this system)"
    run apt-get -y -f install >/dev/null 2>&1 || true
  done
  drop_openresty_repo
  apt_refresh >/dev/null 2>&1
  return 1
}

install_from_rpm_repo() {
  local pm="$1"
  if [ "$pm" = "dnf" ]; then
    run dnf install -y dnf-plugins-core || true
    run dnf config-manager --add-repo https://openresty.org/package/centos/openresty.repo || return 1
  else
    run yum install -y yum-utils || true
    run yum-config-manager --add-repo https://openresty.org/package/centos/openresty.repo || return 1
  fi
  run "$pm" install -y openresty
}

# ---------- source build ----------

install_build_deps() {
  if command -v apt-get >/dev/null 2>&1; then
    # PCRE1 (libpcre3-dev) is gone from Debian 13 / Ubuntu 24.10+; current
    # OpenResty builds against PCRE2.
    local pcre="libpcre2-dev"
    apt_has libpcre2-dev || pcre="libpcre3-dev"
    local deps="build-essential make gcc perl curl tar gzip ca-certificates zlib1g-dev libssl-dev $pcre"
    say "Installing build tools: $deps"
    # shellcheck disable=SC2086
    run apt-get install -y $deps || { apt_refresh; run apt-get install -y $deps; }
  elif command -v dnf >/dev/null 2>&1; then
    run dnf install -y gcc gcc-c++ make perl curl tar gzip pcre2-devel zlib-devel openssl-devel \
      || run dnf install -y gcc gcc-c++ make perl curl tar gzip pcre-devel zlib-devel openssl-devel
  elif command -v yum >/dev/null 2>&1; then
    run yum install -y gcc gcc-c++ make perl curl tar gzip pcre-devel zlib-devel openssl-devel
  else
    say "No supported package manager to install build tools"
    return 1
  fi
}

latest_source_version() {
  curl -fsSL --connect-timeout 15 https://openresty.org/en/download.html 2>/dev/null \
    | grep -Eo 'openresty-[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+\.tar\.gz' \
    | sed -E 's/openresty-(.*)\.tar\.gz/\1/' | sort -uV | tail -n 1
}

install_from_source() {
  say "Building OpenResty from source (takes a few minutes)"
  install_build_deps || true
  local tool
  for tool in make gcc perl; do
    if ! command -v "$tool" >/dev/null 2>&1; then
      say "Build tool '$tool' could not be installed; see the package manager output above"
      return 1
    fi
  done

  local versions latest
  latest="$(latest_source_version)"
  versions="${OPENRESTY_SOURCE_VERSION:-} ${latest} 1.27.1.2 1.25.3.2"
  local work v url src jobs tried=""
  jobs="$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 2)"
  for v in $versions; do
    [ -n "$v" ] || continue
    case " $tried " in *" $v "*) continue ;; esac
    tried="$tried $v"
    url="${OPENRESTY_SOURCE_URL:-https://openresty.org/download/openresty-${v}.tar.gz}"
    work="$(mktemp -d)"
    say "Downloading $url"
    if ! curl -fL --retry 3 --connect-timeout 15 "$url" -o "$work/src.tar.gz"; then
      say "Cannot download $url (is openresty.org reachable from this server?)"
      rm -rf "$work"; continue
    fi
    tar -xzf "$work/src.tar.gz" -C "$work" || { rm -rf "$work"; continue; }
    src="$(find "$work" -maxdepth 1 -type d -name 'openresty-*' | head -n 1)"
    if [ -z "$src" ]; then rm -rf "$work"; continue; fi
    say "Configuring $v"
    if (cd "$src" && run ./configure --prefix="$PREFIX" --with-pcre-jit --with-stream \
          --with-http_v2_module --with-http_ssl_module --with-http_slice_module \
          --with-http_stub_status_module --with-http_sub_module --with-http_realip_module) \
       && say "Compiling $v with $jobs job(s)" && (cd "$src" && run make -j"$jobs") \
       && (cd "$src" && run make install); then
      rm -rf "$work"
      if [ -x "$NGINX_BIN" ]; then
        run ln -sf "$NGINX_BIN" /usr/local/bin/openresty || true
        install_service_unit
        say "Built and installed OpenResty $v"
        return 0
      fi
    fi
    say "Building $v failed"
    rm -rf "$work"
    [ -n "${OPENRESTY_SOURCE_URL:-}" ] && break
  done
  return 1
}

# A source build has no service of its own; without one it would not come
# back after a reboot.
install_service_unit() {
  command -v systemctl >/dev/null 2>&1 || return 0
  [ -d /run/systemd/system ] || return 0
  systemctl cat openresty >/dev/null 2>&1 && return 0
  write_root_file /etc/systemd/system/openresty.service <<UNIT
[Unit]
Description=OpenResty web server (built by LinxPanel)
After=network-online.target
Wants=network-online.target

[Service]
Type=forking
PIDFile=${PREFIX}/nginx/logs/nginx.pid
ExecStartPre=${NGINX_BIN} -t
ExecStart=${NGINX_BIN}
ExecReload=${NGINX_BIN} -s reload
ExecStop=${NGINX_BIN} -s quit
Restart=on-failure

[Install]
WantedBy=multi-user.target
UNIT
  run systemctl daemon-reload || true
}

# ---------- main ----------

say "Starting OpenResty install"
if command -v openresty >/dev/null 2>&1 || [ -x "$NGINX_BIN" ]; then
  say "OpenResty is already installed"
  exit 0
fi

installed=0
if command -v apt-get >/dev/null 2>&1; then
  install_from_apt_repo && installed=1
elif command -v dnf >/dev/null 2>&1; then
  install_from_rpm_repo dnf && installed=1
elif command -v yum >/dev/null 2>&1; then
  install_from_rpm_repo yum && installed=1
fi

if [ "$installed" != "1" ]; then
  say "No usable OpenResty package for this system; falling back to a source build"
  if ! install_from_source; then
    say "OpenResty source fallback failed"
    exit 51
  fi
fi

if command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ]; then
  run systemctl enable openresty >/dev/null 2>&1 || true
  run systemctl start openresty || true
fi
say "OpenResty install completed"
