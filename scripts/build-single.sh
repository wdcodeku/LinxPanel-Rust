#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
LANGUAGE="${LINXPANEL_LANG:-en}"

normalize_lang() {
  local value="${1:-}"
  value="$(printf '%s' "${value}" | tr '[:upper:]' '[:lower:]')"
  case "${value}" in
    2 | zh | zh-cn | zh_cn | cn | chinese | 中文) echo "zh" ;;
    *) echo "en" ;;
  esac
}

LANGUAGE="$(normalize_lang "${LANGUAGE}")"

msg() {
  local key="$1"
  if [[ "${LANGUAGE}" == "zh" ]]; then
    case "${key}" in
      build_web) echo "正在构建前端资源..." ;;
      build_backend) echo "正在构建后端 release 二进制..." ;;
      binary_missing) echo "构建完成但未找到二进制文件：%s" ;;
      done) echo "完成。单文件可部署二进制：" ;;
      *) echo "${key}" ;;
    esac
  else
    case "${key}" in
      build_web) echo "Building web assets..." ;;
      build_backend) echo "Building backend release binary..." ;;
      binary_missing) echo "build completed but binary not found: %s" ;;
      done) echo "Done. Single deployable file:" ;;
      *) echo "${key}" ;;
    esac
  fi
}

logf() {
  local key="$1"
  shift
  local template
  template="$(msg "${key}")"
  printf "[LinxPanel] "
  printf "${template}\n" "$@"
}

command -v cargo >/dev/null 2>&1 || { echo "cargo not found in PATH"; exit 1; }
command -v npm >/dev/null 2>&1 || { echo "npm not found in PATH"; exit 1; }

CARGO_BIN=(cargo)
if command -v rustup >/dev/null 2>&1; then
  CARGO_BIN=(cargo +stable)
fi

logf build_web
cd "${REPO_ROOT}/web"
if [[ -f package-lock.json ]]; then
  npm ci
else
  npm install
fi
npm run build

logf build_backend
cd "${REPO_ROOT}"
"${CARGO_BIN[@]}" build --release

BINARY_PATH="${REPO_ROOT}/target/release/linxpanel"
if [[ ! -f "${BINARY_PATH}" ]]; then
  logf binary_missing "${BINARY_PATH}"
  exit 1
fi

logf done
echo "  ${BINARY_PATH}"
