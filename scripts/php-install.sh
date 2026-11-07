#!/usr/bin/env bash
# Installs or removes one PHP version for LinxPanel, side by side with others.
# Debian / Ubuntu only: versions come from Ondřej Surý's packages (the Ubuntu
# PPA, or packages.sury.org on Debian), with mirrors as fallback when the
# official host is unreachable. Run by the panel as root.
#
#   PHP_ACTION=install PHP_VERSION=8.3 [PHP_EXTENSIONS="redis imagick"]
#   PHP_ACTION=remove  PHP_VERSION=7.4
#   PHP_ACTION=repo                      (only add the repository)
set -u

say() { echo "[LinxPanel] $*"; }

ACTION="${PHP_ACTION:-install}"
VERSION="${PHP_VERSION:-}"
case "${ACTION}" in
  install | remove)
    case "${VERSION}" in
      5.6 | 7.0 | 7.1 | 7.2 | 7.3 | 7.4 | 8.0 | 8.1 | 8.2 | 8.3 | 8.4 | 8.5) ;;
      *) say "Unsupported PHP version: ${VERSION:-none}"; exit 2 ;;
    esac ;;
  repo) ;;
  *) say "Unknown action: ${ACTION}"; exit 2 ;;
esac

run() {
  if [ "$1" = "apt-get" ]; then
    shift
    set -- apt-get -o DPkg::Lock::Timeout=600 \
      -o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold "$@"
  fi
  env DEBIAN_FRONTEND=noninteractive NEEDRESTART_MODE=a NEEDRESTART_SUSPEND=1 "$@"
}

if ! command -v apt-get >/dev/null 2>&1; then
  say "Multiple PHP versions are supported on Debian and Ubuntu; this system uses its distribution's PHP"
  exit 3
fi

id="" codename=""
# Read in subshells: sourcing os-release here would overwrite our own
# variables (it defines VERSION, among others).
os_field() { [ -r /etc/os-release ] && (. /etc/os-release && eval "printf '%s' \"\${$1:-}\""); }
id="$(os_field ID)"
codename="$(os_field VERSION_CODENAME)"
[ -n "${codename}" ] || codename="$(os_field UBUNTU_CODENAME)"

reachable() { curl -fsS --max-time 15 -o /dev/null "$1"; }

has_candidate() {
  apt-cache policy "$1" 2>/dev/null | grep -q 'Candidate: [0-9]'
}

KEYRING="/usr/share/keyrings/linxpanel-php.gpg"
LIST="/etc/apt/sources.list.d/linxpanel-php.list"
# Ondřej Surý's PPA signing key.
PPA_KEY_FPR="14AA40EC0831756756D7F66C4F4EA0AAE5267A6C"

ensure_repo() {
  if [ -s "${LIST}" ]; then
    return 0
  fi
  run apt-get install -y curl gnupg ca-certificates >/dev/null 2>&1 || true
  local tmp bases base dist_url
  tmp="$(mktemp)"
  case "${id}" in
    ubuntu)
      if ! curl -fsSL --retry 3 --connect-timeout 15 \
        "https://keyserver.ubuntu.com/pks/lookup?op=get&options=mr&search=0x${PPA_KEY_FPR}" -o "${tmp}"; then
        say "Cannot download the PHP repository key (keyserver.ubuntu.com unreachable)"
        rm -f "${tmp}"; return 1
      fi
      bases="https://ppa.launchpadcontent.net/ondrej/php/ubuntu https://launchpad.proxy.ustclug.org/ondrej/php/ubuntu"
      ;;
    debian)
      if ! curl -fsSL --retry 3 --connect-timeout 15 "https://packages.sury.org/php/apt.gpg" -o "${tmp}" \
        && ! curl -fsSL --retry 3 --connect-timeout 15 "https://mirrors.ustc.edu.cn/sury/php/apt.gpg" -o "${tmp}"; then
        say "Cannot download the PHP repository key (packages.sury.org unreachable)"
        rm -f "${tmp}"; return 1
      fi
      bases="https://packages.sury.org/php https://mirrors.ustc.edu.cn/sury/php"
      ;;
    *)
      say "No multi-version PHP packages for ${id:-this system}"
      rm -f "${tmp}"; return 1 ;;
  esac
  # An ASCII-armored key needs dearmoring; a binary keyring is used as is.
  if grep -q "BEGIN PGP" "${tmp}"; then
    run gpg --batch --yes --dearmor -o "${KEYRING}" "${tmp}" || { rm -f "${tmp}"; return 1; }
  else
    install -m 0644 "${tmp}" "${KEYRING}"
  fi
  rm -f "${tmp}"
  for base in ${bases}; do
    dist_url="${base}/dists/${codename}/Release"
    if reachable "${dist_url}"; then
      say "Using ${base} (${codename})"
      echo "deb [signed-by=${KEYRING}] ${base} ${codename} main" >"${LIST}"
      run apt-get update || say "apt-get update reported errors (often an unrelated broken source); continuing"
      return 0
    fi
  done
  say "No PHP repository has packages for ${id} ${codename}"
  return 1
}

# Extensions most sites need; each is installed only when this version has it.
COMMON="mysql pgsql sqlite3 curl gd mbstring xml zip intl bcmath opcache redis imagick soap gmp"

# The distribution's unversioned metapackages (php-fpm, php-mysql...) follow
# the repository's newest PHP once its php-common is installed, which would
# quietly add PHP 8.x versions nobody asked for. Keep every versioned package
# that is installed now, then let the metapackages go.
detach_metapackages() {
  local versioned metas
  versioned="$(dpkg-query -W -f='${Package} ${Status}\n' 'php[0-9]*' 2>/dev/null | awk '/ installed$/ {print $1}')"
  if [ -n "${versioned}" ]; then
    # shellcheck disable=SC2086
    apt-mark manual ${versioned} >/dev/null 2>&1 || true
  fi
  metas="$(dpkg-query -W -f='${Package} ${Status}\n' 'php-*' 2>/dev/null | awk '/ installed$/ {print $1}' \
    | grep -E '^php-(bcmath|cli|curl|fpm|gd|intl|mbstring|mysql|pgsql|redis|sqlite3|xml|zip|opcache|soap|gmp|imagick|igbinary|readline|cgi|ldap|imap|memcached|apcu|xdebug|dev)$' || true)"
  if [ -n "${metas}" ]; then
    say "Detaching distribution metapackages: $(echo ${metas} | tr '\n' ' ')"
    # shellcheck disable=SC2086
    run apt-get remove -y ${metas} >/dev/null 2>&1 || true
  fi
}

do_install() {
  if ! has_candidate "php${VERSION}-fpm"; then
    ensure_repo || exit 1
  fi
  detach_metapackages
  if ! has_candidate "php${VERSION}-fpm"; then
    say "PHP ${VERSION} is not available for ${id} ${codename}"
    exit 1
  fi
  local packages="php${VERSION}-fpm php${VERSION}-cli php${VERSION}-common"
  local ext
  for ext in ${COMMON} ${PHP_EXTENSIONS:-}; do
    if has_candidate "php${VERSION}-${ext}"; then
      packages="${packages} php${VERSION}-${ext}"
    fi
  done
  say "Installing PHP ${VERSION}: ${packages}"
  # shellcheck disable=SC2086
  run apt-get install -y --no-install-recommends ${packages} || { say "PHP ${VERSION} install failed"; exit 1; }
  local pool="/etc/php/${VERSION}/fpm/pool.d/www.conf"
  if [ -f "${pool}" ]; then
    # OpenResty workers are not www-data: open the socket to local users.
    sed -i -e 's/^\s*listen\.acl_\(users\|groups\)/;&/' "${pool}"
    if grep -q '^\s*;\?\s*listen\.mode' "${pool}"; then
      sed -i -e 's/^\s*;\?\s*listen\.mode\s*=.*/listen.mode = 0666/' "${pool}"
    else
      echo "listen.mode = 0666" >>"${pool}"
    fi
  fi
  if command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ]; then
    systemctl enable "php${VERSION}-fpm" >/dev/null 2>&1 || true
    systemctl restart "php${VERSION}-fpm" || { say "php${VERSION}-fpm did not start"; exit 1; }
  else
    service "php${VERSION}-fpm" restart || { say "php${VERSION}-fpm did not start"; exit 1; }
  fi
  say "PHP ${VERSION} is ready: $(php${VERSION} -r 'echo PHP_VERSION;' 2>/dev/null)"
}

do_remove() {
  local packages
  packages="$(dpkg-query -W -f='${Package} ${Status}\n' "php${VERSION}-*" 2>/dev/null | awk '/ installed$/ {print $1}' | tr '\n' ' ')"
  if [ -z "${packages}" ]; then
    say "PHP ${VERSION} is not installed"
    exit 0
  fi
  say "Removing ${packages}"
  # shellcheck disable=SC2086
  run apt-get purge -y ${packages} || { say "Removing PHP ${VERSION} failed"; exit 1; }
  run apt-get autoremove -y >/dev/null 2>&1 || true
  say "PHP ${VERSION} removed"
}

case "${ACTION}" in
  repo) ensure_repo ;;
  install) do_install ;;
  remove) do_remove ;;
esac
