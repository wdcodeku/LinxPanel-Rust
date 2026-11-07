#!/usr/bin/env bash
# Adds the official MySQL or MariaDB package repository for one release
# series, so `mysql-server` / `mariadb-server` install that series instead of
# the distribution's. Debian / Ubuntu only, mirrors as fallback when the
# official host is unreachable. Run by the panel as root.
#
#   DB_FLAVOR=mysql   DB_SERIES=8.4 | 9.7
#   DB_FLAVOR=mariadb DB_SERIES=10.11 | 11.4 | 11.8 | 12.3
#   DB_FLAVOR=none    (remove the repository the panel added)
set -u

say() { echo "[LinxPanel] $*"; }

FLAVOR="${DB_FLAVOR:-}"
SERIES="${DB_SERIES:-}"

run() {
  if [ "$1" = "apt-get" ]; then
    shift
    set -- apt-get -o DPkg::Lock::Timeout=600 "$@"
  fi
  env DEBIAN_FRONTEND=noninteractive "$@"
}

remove_repos() {
  rm -f /etc/apt/sources.list.d/linxpanel-mysql.list /etc/apt/sources.list.d/linxpanel-mariadb.list \
    /etc/apt/preferences.d/linxpanel-mysql /etc/apt/preferences.d/linxpanel-mariadb \
    /usr/share/keyrings/linxpanel-mysql.gpg /usr/share/keyrings/linxpanel-mariadb.gpg
}

case "${FLAVOR}:${SERIES}" in
  mysql:8.4 | mysql:9.7 | mariadb:10.11 | mariadb:11.4 | mariadb:11.8 | mariadb:12.3) ;;
  none:*) remove_repos; run apt-get update >/dev/null 2>&1 || true; say "Database repository removed"; exit 0 ;;
  *) say "Unsupported version: ${FLAVOR} ${SERIES}"; exit 2 ;;
esac

if ! command -v apt-get >/dev/null 2>&1; then
  say "Official ${FLAVOR} repositories are supported on Debian and Ubuntu"
  exit 3
fi

# Read in subshells: sourcing os-release here would overwrite our variables.
os_field() { [ -r /etc/os-release ] && (. /etc/os-release && eval "printf '%s' \"\${$1:-}\""); }
id="$(os_field ID)"
codename="$(os_field VERSION_CODENAME)"
[ -n "${codename}" ] || codename="$(os_field UBUNTU_CODENAME)"
case "${id}" in
  ubuntu | debian) ;;
  *) say "No official ${FLAVOR} packages for ${id:-this system}"; exit 3 ;;
esac
arch="$(dpkg --print-architecture)"

reachable() { curl -fsS --max-time 15 -o /dev/null "$1"; }

run apt-get install -y curl gnupg ca-certificates >/dev/null 2>&1 || true

if [ "${FLAVOR}" = "mysql" ]; then
  # Oracle builds Debian / Ubuntu packages for x86-64 only.
  if [ "${arch}" != "amd64" ]; then
    say "Oracle publishes MySQL ${SERIES} packages for amd64 only (this server is ${arch}); choose MariaDB or the distribution's MySQL"
    exit 3
  fi
  component="mysql-${SERIES}-lts"
  # The 2023 key with its expiry extended to 2027 (the -2023 file on
  # repo.mysql.com still carries the old, expired date).
  key_urls="https://repo.mysql.com/RPM-GPG-KEY-mysql-2025 https://keyserver.ubuntu.com/pks/lookup?op=get&options=mr&search=0xBCA43417C3B485DD128EC6D4B7B3B788A8D3785C"
  bases="https://repo.mysql.com/apt/${id} https://mirrors.tuna.tsinghua.edu.cn/mysql/apt/${id} https://mirrors.ustc.edu.cn/mysql-repo/apt/${id}"
  origin="MySQL"
  packages_hint="mysql-server"
else
  component="main"
  key_urls="https://mariadb.org/mariadb_release_signing_key.pgp https://keyserver.ubuntu.com/pks/lookup?op=get&options=mr&search=0x177F4010FE56CA3336300305F1656F24C74CD1D8"
  bases="https://dlm.mariadb.com/repo/mariadb-server/${SERIES}/repo/${id} https://mirrors.aliyun.com/mariadb/repo/${SERIES}/${id} https://mirrors.tuna.tsinghua.edu.cn/mariadb/repo/${SERIES}/${id}"
  origin="MariaDB"
  packages_hint="mariadb-server"
fi

KEYRING="/usr/share/keyrings/linxpanel-${FLAVOR}.gpg"
LIST="/etc/apt/sources.list.d/linxpanel-${FLAVOR}.list"
PIN="/etc/apt/preferences.d/linxpanel-${FLAVOR}"

tmp="$(mktemp)"
got_key=0
for url in ${key_urls}; do
  if curl -fsSL --retry 3 --connect-timeout 15 "${url}" -o "${tmp}" && [ -s "${tmp}" ]; then
    got_key=1
    break
  fi
done
if [ "${got_key}" != 1 ]; then
  say "Cannot download the ${origin} signing key"
  rm -f "${tmp}"
  exit 1
fi

chosen=""
for base in ${bases}; do
  if reachable "${base}/dists/${codename}/Release"; then
    chosen="${base}"
    break
  fi
done
if [ -z "${chosen}" ]; then
  say "${origin} ${SERIES} has no packages for ${id} ${codename}"
  rm -f "${tmp}"
  exit 1
fi

# Only one database repository at a time: MySQL and MariaDB do not mix.
remove_repos
if grep -q "BEGIN PGP" "${tmp}"; then
  gpg --batch --yes --dearmor -o "${KEYRING}" "${tmp}" || { rm -f "${tmp}"; say "Invalid ${origin} signing key"; exit 1; }
else
  install -m 0644 "${tmp}" "${KEYRING}"
fi
chmod 0644 "${KEYRING}"
rm -f "${tmp}"

say "Using ${chosen} (${codename}, ${component})"
echo "deb [arch=${arch} signed-by=${KEYRING}] ${chosen} ${codename} ${component}" >"${LIST}"
# Prefer this repository over the distribution's packages of the same name.
cat >"${PIN}" <<EOF
Package: *
Pin: release o=${origin}
Pin-Priority: 1000
EOF

if ! run apt-get update; then
  say "apt-get update reported errors (often an unrelated broken source); continuing"
fi
candidate="$(apt-cache policy "${packages_hint}" 2>/dev/null | awk '/Candidate:/ {print $2}')"
case "${candidate}" in
  "" | "(none)")
    remove_repos
    say "${origin} ${SERIES} packages are not available for ${id} ${codename}"
    exit 1 ;;
esac
say "${packages_hint} will install ${candidate}"

# Shared packages a previous MySQL / MariaDB left behind (mysql-common,
# client libraries) can carry a higher version than this repository's,
# e.g. MariaDB's epoch "1:". Install this repository's versions now, so the
# server install does not stop at "packages were downgraded".
align=""
for pkg in $(dpkg-query -W -f='${Package} ${db:Status-Abbrev}\n' 'mysql-*' 'mariadb-*' 'libmysqlclient*' 'libmariadb*' 2>/dev/null | awk '$2 ~ /^ii/ {print $1}'); do
  installed="$(dpkg-query -W -f='${Version}' "${pkg}" 2>/dev/null)"
  cand="$(apt-cache policy "${pkg}" 2>/dev/null | awk '/Candidate:/ {print $2}')"
  case "${cand}" in "" | "(none)") continue ;; esac
  if [ "${cand}" != "${installed}" ] && dpkg --compare-versions "${cand}" lt "${installed}"; then
    align="${align} ${pkg}"
  fi
done
# MySQL reads /etc/mysql/my.cnf, which MariaDB's leftover mariadb-common
# (a higher alternatives priority) would point at its own files: install
# MySQL's mysql-common now and make its my.cnf the active one.
if [ "${FLAVOR}" = "mysql" ]; then
  case " ${align} " in *" mysql-common "*) ;; *) align="${align} mysql-common" ;; esac
fi
if [ -n "${align}" ]; then
  say "Using ${origin}'s versions of:${align}"
  # shellcheck disable=SC2086
  run apt-get install -y --allow-downgrades -o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold ${align} \
    || say "Could not switch${align}; the install may stop on them"
fi
installed() { dpkg-query -W -f='${db:Status-Abbrev}' "$1" 2>/dev/null | grep -q '^ii'; }
# Users of a package other than MariaDB's own client pieces.
other_users() {
  apt-cache rdepends --installed "$1" 2>/dev/null | tail -n +3 | tr -d ' |' \
    | grep -v -x -E 'libmariadb3|mariadb-common|mysql-common|galera-4' || true
}
if [ "${FLAVOR}" = "mysql" ] && ! installed mariadb-server && installed mariadb-common; then
  # A MariaDB that was here before left its client pieces and configuration
  # (mariadb.conf.d) behind; MySQL would read them through my.cnf and stop
  # at MariaDB-only options.
  if [ -z "$(other_users mariadb-common)" ] && [ -z "$(other_users libmariadb3)" ]; then
    say "Removing what the previous MariaDB left behind"
    run apt-get purge -y mariadb-common libmariadb3 galera-4 >/dev/null 2>&1 || true
    rm -f /etc/mysql/mariadb.conf.d/feedback.cnf
    rmdir /etc/mysql/mariadb.conf.d 2>/dev/null || true
  else
    # Still used by other software: keep it, but not as the server's my.cnf.
    update-alternatives --set my.cnf /etc/mysql/my.cnf.fallback >/dev/null 2>&1 || true
  fi
elif [ "${FLAVOR}" = "mariadb" ]; then
  update-alternatives --auto my.cnf >/dev/null 2>&1 || true
fi
