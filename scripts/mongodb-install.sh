#!/usr/bin/env bash
# Installs MongoDB Community for LinxPanel from MongoDB's own package
# repository (distributions no longer ship it), with a mirror as fallback.
# Run by the panel as root; MONGODB_VERSION picks the series (8.0 / 7.0).
set -u

say() { echo "[LinxPanel] $*"; }

VERSION="${MONGODB_VERSION:-8.0}"
case "${VERSION}" in
  8.0 | 7.0) ;;
  *) say "Unsupported MongoDB version: ${VERSION}"; exit 2 ;;
esac

run() {
  if [ "$1" = "apt-get" ]; then
    shift
    set -- apt-get -o DPkg::Lock::Timeout=600 \
      -o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold "$@"
  fi
  env DEBIAN_FRONTEND=noninteractive NEEDRESTART_MODE=a NEEDRESTART_SUSPEND=1 "$@"
}

# MongoDB 5.0 and later need AVX on x86-64; without it mongod dies at start
# with "Illegal instruction" - say so plainly instead.
arch="$(uname -m)"
if [ "${arch}" = "x86_64" ] && ! grep -qw avx /proc/cpuinfo; then
  say "This CPU has no AVX instructions, which MongoDB ${VERSION} requires (common on older or emulated VPS CPUs)."
  say "Ask the provider for a CPU model with AVX, or use another database."
  exit 3
fi
case "${arch}" in
  x86_64 | aarch64) ;;
  *) say "MongoDB has no packages for ${arch}"; exit 3 ;;
esac

if command -v mongod >/dev/null 2>&1; then
  say "MongoDB is already installed: $(mongod --version 2>/dev/null | head -n1)"
  installed=1
else
  installed=0
fi

id="" codename="" version_id=""
if [ -r /etc/os-release ]; then
  # shellcheck disable=SC1091
  . /etc/os-release
  id="${ID:-}"
  codename="${VERSION_CODENAME:-}"
  version_id="${VERSION_ID:-}"
fi
say "System: ${PRETTY_NAME:-unknown}"

KEYRING="/usr/share/keyrings/mongodb-server-${VERSION}.gpg"
LIST="/etc/apt/sources.list.d/mongodb-org-${VERSION}.list"
OFFICIAL="https://repo.mongodb.org"
MIRROR="https://mirrors.aliyun.com/mongodb"

reachable() {
  curl -fsS --max-time 15 -o /dev/null "$1"
}

apt_install() {
  local flavor component candidates base c
  case "${id}" in
    ubuntu)
      flavor="ubuntu"; component="multiverse"
      candidates="${codename} noble jammy focal" ;;
    debian)
      flavor="debian"; component="main"
      candidates="${codename} bookworm bullseye" ;;
    *)
      say "No MongoDB packages for ${id:-this system}"
      return 1 ;;
  esac
  run apt-get install -y curl gnupg ca-certificates >/dev/null 2>&1 || true

  local tmp_key
  tmp_key="$(mktemp)"
  if ! curl -fsSL --retry 3 --connect-timeout 15 "https://pgp.mongodb.com/server-${VERSION}.asc" -o "${tmp_key}" \
    && ! curl -fsSL --retry 3 --connect-timeout 15 "https://www.mongodb.org/static/pgp/server-${VERSION}.asc" -o "${tmp_key}"; then
    say "Cannot download MongoDB's signing key (pgp.mongodb.com / www.mongodb.org unreachable)"
    rm -f "${tmp_key}"
    return 1
  fi
  run gpg --batch --yes --dearmor -o "${KEYRING}" "${tmp_key}" || { rm -f "${tmp_key}"; return 1; }
  rm -f "${tmp_key}"

  local tried=""
  for c in ${candidates}; do
    [ -n "${c}" ] || continue
    case " ${tried} " in *" ${c} "*) continue ;; esac
    tried="${tried} ${c}"
    for base in "${OFFICIAL}" "${MIRROR}"; do
      if ! reachable "${base}/apt/${flavor}/dists/${c}/mongodb-org/${VERSION}/Release"; then
        continue
      fi
      say "Using ${base} (${c})"
      echo "deb [arch=amd64,arm64 signed-by=${KEYRING}] ${base}/apt/${flavor} ${c}/mongodb-org/${VERSION} ${component}" >"${LIST}"
      run apt-get update || say "apt-get update reported errors (often an unrelated broken source); continuing"
      if run apt-get install -y mongodb-org; then
        return 0
      fi
      say "Installing from ${base} (${c}) failed"
    done
  done
  rm -f "${LIST}"
  run apt-get update >/dev/null 2>&1 || true
  say "No MongoDB ${VERSION} repository worked for this system"
  return 1
}

rpm_install() {
  local manager="$1" major base
  major="${version_id%%.*}"
  for base in "${OFFICIAL}" "${MIRROR}"; do
    if reachable "${base}/yum/redhat/${major}/mongodb-org/${VERSION}/x86_64/repodata/repomd.xml" \
      || reachable "${base}/yum/redhat/${major}/mongodb-org/${VERSION}/aarch64/repodata/repomd.xml"; then
      say "Using ${base}"
      cat >"/etc/yum.repos.d/mongodb-org-${VERSION}.repo" <<REPO
[mongodb-org-${VERSION}]
name=MongoDB Repository
baseurl=${base}/yum/redhat/\$releasever/mongodb-org/${VERSION}/\$basearch/
gpgcheck=1
enabled=1
gpgkey=https://pgp.mongodb.com/server-${VERSION}.asc
REPO
      run "${manager}" install -y mongodb-org && return 0
    fi
  done
  rm -f "/etc/yum.repos.d/mongodb-org-${VERSION}.repo"
  return 1
}

if [ "${installed}" = "0" ]; then
  if command -v apt-get >/dev/null 2>&1; then
    apt_install || { say "MongoDB install failed"; exit 1; }
  elif command -v dnf >/dev/null 2>&1; then
    rpm_install dnf || { say "MongoDB install failed"; exit 1; }
  elif command -v yum >/dev/null 2>&1; then
    rpm_install yum || { say "MongoDB install failed"; exit 1; }
  else
    say "No supported package manager"
    exit 1
  fi
fi

if command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ]; then
  systemctl daemon-reload || true
  systemctl enable mongod >/dev/null 2>&1 || true
  systemctl restart mongod || { say "mongod did not start"; journalctl -u mongod -n 20 --no-pager 2>/dev/null; exit 1; }
else
  service mongod restart || { say "mongod did not start"; exit 1; }
fi
say "MongoDB $(mongod --version 2>/dev/null | head -n1 | sed 's/.*version //') is running"
