#!/usr/bin/env bash
set -euo pipefail

APP_ID="dev.witty.Witty"
APP_NAME="Witty"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
PREFIX="${WITTY_INSTALL_PREFIX:-${HOME:?HOME must be set}/.local}"
if [[ -n "${XDG_STATE_HOME:-}" ]]; then
  STATE_HOME="${XDG_STATE_HOME}"
else
  STATE_HOME="${HOME:?HOME must be set}/.local/state"
fi
DRY_RUN=0
BUILD=1
BUILD_PROFILE="debug"
BINARY_SOURCE=""
BINARY_OVERRIDE=0
ICON_SIZES=(16 24 32 48 64 128 256 512)

usage() {
  cat <<'MSG'
Usage: scripts/install-witty-local.sh [options]

Installs Witty into a user-local Linux prefix. By default this builds the
debug witty binary and installs under ~/.local.

Options:
  --dry-run, --print-plan  Print deterministic install targets without building or writing.
  --prefix PATH            Install under PATH instead of ~/.local.
  --binary PATH            Install an already-built witty binary instead of building.
  --debug                  Use target/debug/witty when building (default).
  --release                Build and install target/release/witty.
  --no-build               Do not run cargo build; require the target binary to exist.
  --build                  Run cargo build before installing (default unless --binary is used).
  -h, --help               Show this help text.

Environment:
  WITTY_INSTALL_PREFIX      Default install prefix override.
  XDG_STATE_HOME            State directory override for install-state.v1.json.
MSG
}

fail() {
  printf 'install-witty-local: %s\n' "$*" >&2
  exit 1
}

absolute_path() {
  local path="$1"
  case "${path}" in
    /*) printf '%s\n' "${path}" ;;
    *) printf '%s/%s\n' "$(pwd)" "${path}" ;;
  esac
}

shell_words() {
  local first=1
  local part
  for part in "$@"; do
    if [[ "${first}" == "0" ]]; then
      printf ' '
    fi
    printf '%q' "${part}"
    first=0
  done
}

desktop_exec_arg() {
  local value="$1"
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  value="${value//\$/\\$}"
  value="${value//\`/\\\`}"
  printf '"%s"' "${value}"
}

json_string() {
  local value="$1"
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  value="${value//$'\n'/\\n}"
  value="${value//$'\r'/\\r}"
  value="${value//$'\t'/\\t}"
  printf '"%s"' "${value}"
}

workspace_package_version() {
  awk '
    /^\[workspace.package\]/ { in_section = 1; next }
    /^\[/ { in_section = 0 }
    in_section && $1 == "version" {
      value = $3
      gsub(/"/, "", value)
      print value
      exit
    }
  ' "${ROOT_DIR}/Cargo.toml"
}

binary_sha256() {
  local path="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${path}" | awk '{ print $1 }'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${path}" | awk '{ print $1 }'
  else
    printf 'sha256-unavailable'
  fi
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run | --print-plan)
      DRY_RUN=1
      shift
      ;;
    --prefix)
      [[ $# -ge 2 ]] || fail "--prefix requires a path"
      PREFIX="$2"
      shift 2
      ;;
    --binary)
      [[ $# -ge 2 ]] || fail "--binary requires a path"
      BINARY_SOURCE="$(absolute_path "$2")"
      BINARY_OVERRIDE=1
      shift 2
      ;;
    --debug)
      BUILD_PROFILE="debug"
      shift
      ;;
    --release)
      BUILD_PROFILE="release"
      shift
      ;;
    --no-build)
      BUILD=0
      shift
      ;;
    --build)
      BUILD=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      fail "unknown option $1"
      ;;
  esac
done

PREFIX="$(absolute_path "${PREFIX}")"
if [[ "${BINARY_OVERRIDE}" == "1" && "${BUILD}" == "1" ]]; then
  BUILD=0
fi

if [[ -z "${BINARY_SOURCE}" ]]; then
  if [[ "${BUILD_PROFILE}" == "release" ]]; then
    BINARY_SOURCE="${ROOT_DIR}/target/release/witty"
  else
    BINARY_SOURCE="${ROOT_DIR}/target/debug/witty"
  fi
fi

BIN_TARGET="${PREFIX}/bin/witty"
DESKTOP_TARGET="${PREFIX}/share/applications/${APP_ID}.desktop"
ICON_BASE="${PREFIX}/share/icons/hicolor"
SVG_SOURCE="${ROOT_DIR}/assets/icon.svg"
SVG_TARGET="${ICON_BASE}/scalable/apps/${APP_ID}.svg"
INSTALL_STATE_TARGET="${STATE_HOME}/witty/install-state.v1.json"
BUILD_COMMAND=(cargo build -p witty-app)
if [[ "${BUILD_PROFILE}" == "release" ]]; then
  BUILD_COMMAND+=(--release)
fi

desktop_exec_line() {
  printf '/usr/bin/env WGPU_BACKEND=gl '
  desktop_exec_arg "${BIN_TARGET}"
  printf ' --window'
}

print_plan() {
  printf 'Witty local install plan\n'
  printf '  repository: %s\n' "${ROOT_DIR}"
  printf '  prefix: %s\n' "${PREFIX}"
  if [[ "${BUILD}" == "1" ]]; then
    printf '  build: '
    shell_words "${BUILD_COMMAND[@]}"
    printf '\n'
  else
    printf '  build: skipped\n'
  fi
  printf '  binary source: %s\n' "${BINARY_SOURCE}"
  printf '  binary target: %s\n' "${BIN_TARGET}"
  printf '  desktop target: %s\n' "${DESKTOP_TARGET}"
  printf '  install state target: %s\n' "${INSTALL_STATE_TARGET}"
  printf '  desktop Exec: %s\n' "$(desktop_exec_line)"
  printf '  desktop Icon: %s\n' "${APP_ID}"
  printf '  desktop StartupWMClass: %s\n' "${APP_ID}"
  printf '  icon targets:\n'
  local size
  for size in "${ICON_SIZES[@]}"; do
    printf '    %s\n' "${ICON_BASE}/${size}x${size}/apps/${APP_ID}.png"
  done
  printf '    %s\n' "${SVG_TARGET}"
}

validate_assets() {
  local size
  for size in "${ICON_SIZES[@]}"; do
    local source="${ROOT_DIR}/assets/icons/icon_${size}x${size}.png"
    [[ -f "${source}" ]] || fail "missing icon asset ${source}"
  done
  [[ -f "${SVG_SOURCE}" ]] || fail "missing icon asset ${SVG_SOURCE}"
}

write_desktop_file() {
  local target="$1"
  mkdir -p "$(dirname "${target}")"
  {
    printf '[Desktop Entry]\n'
    printf 'Type=Application\n'
    printf 'Name=%s\n' "${APP_NAME}"
    printf 'GenericName=Terminal Emulator\n'
    printf 'Comment=Witty terminal using the wgpu OpenGL backend\n'
    printf 'Exec=%s\n' "$(desktop_exec_line)"
    printf 'Icon=%s\n' "${APP_ID}"
    printf 'Terminal=false\n'
    printf 'Categories=System;TerminalEmulator;\n'
    printf 'Keywords=shell;prompt;command;terminal;witty;\n'
    printf 'StartupNotify=true\n'
    printf 'StartupWMClass=%s\n' "${APP_ID}"
  } > "${target}"
  chmod 0644 "${target}"
}

write_install_state_file() {
  local target="$1"
  local package_version
  package_version="$(workspace_package_version)"
  [[ -n "${package_version}" ]] || package_version="0.1.0"

  local installed_at_utc
  installed_at_utc="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  local binary_hash
  binary_hash="$(binary_sha256 "${BIN_TARGET}")"
  local source_profile="${BUILD_PROFILE}"
  if [[ "${BINARY_OVERRIDE}" == "1" ]]; then
    source_profile="binary"
  fi
  local build_id="witty:${package_version}:${binary_hash}:${installed_at_utc}"
  local temp="${target}.tmp.$$"

  mkdir -p "$(dirname "${target}")"
  {
    printf '{\n'
    printf '  "schema_version": 1,\n'
    printf '  "app_id": %s,\n' "$(json_string "${APP_ID}")"
    printf '  "build_id": %s,\n' "$(json_string "${build_id}")"
    printf '  "package_version": %s,\n' "$(json_string "${package_version}")"
    printf '  "installed_at_utc": %s,\n' "$(json_string "${installed_at_utc}")"
    printf '  "binary_path": %s,\n' "$(json_string "${BIN_TARGET}")"
    printf '  "install_prefix": %s,\n' "$(json_string "${PREFIX}")"
    printf '  "source_profile": %s\n' "$(json_string "${source_profile}")"
    printf '}\n'
  } > "${temp}"
  chmod 0644 "${temp}"
  mv -f "${temp}" "${target}"
}

if [[ "${DRY_RUN}" == "1" ]]; then
  print_plan
  exit 0
fi

validate_assets

if [[ "${BUILD}" == "1" ]]; then
  printf 'Building Witty: '
  shell_words "${BUILD_COMMAND[@]}"
  printf '\n'
  (cd "${ROOT_DIR}" && "${BUILD_COMMAND[@]}")
fi

[[ -x "${BINARY_SOURCE}" ]] || fail "binary source is not executable: ${BINARY_SOURCE}"

install -D -m 0755 "${BINARY_SOURCE}" "${BIN_TARGET}"

for size in "${ICON_SIZES[@]}"; do
  install -D -m 0644 \
    "${ROOT_DIR}/assets/icons/icon_${size}x${size}.png" \
    "${ICON_BASE}/${size}x${size}/apps/${APP_ID}.png"
done
install -D -m 0644 "${SVG_SOURCE}" "${SVG_TARGET}"

write_desktop_file "${DESKTOP_TARGET}"

if command -v desktop-file-validate >/dev/null 2>&1; then
  desktop-file-validate "${DESKTOP_TARGET}"
  printf 'desktop-file-validate: passed %s\n' "${DESKTOP_TARGET}"
else
  printf 'desktop-file-validate: optional tool not found; skipped validation for %s\n' \
    "${DESKTOP_TARGET}" >&2
fi

write_install_state_file "${INSTALL_STATE_TARGET}"

printf 'Installed %s to %s\n' "${APP_NAME}" "${PREFIX}"
printf 'Wrote install state marker %s\n' "${INSTALL_STATE_TARGET}"
printf 'Launch from GNOME with desktop id %s, or run: %s --window\n' \
  "${APP_ID}" "${BIN_TARGET}"
