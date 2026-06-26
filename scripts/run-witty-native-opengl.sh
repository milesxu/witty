#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WITTY_NATIVE_BIN="${WITTY_NATIVE_BIN:-}"
DEBUG_BIN="${ROOT_DIR}/target/debug/witty"
PRINT_COMMAND=0

usage() {
  cat <<'MSG'
Usage: scripts/run-witty-native-opengl.sh [--print-command] [window args...]
       scripts/run-witty-native-opengl.sh [--print-command] --font-list [filter]
       scripts/run-witty-native-opengl.sh [--print-command] --wittyrc-template
       scripts/run-witty-native-opengl.sh [--print-command] --wittyrc-default-path
       scripts/run-witty-native-opengl.sh [--print-command] --wittyrc-init [config args...]
       scripts/run-witty-native-opengl.sh [--print-command] --wittyrc-check [config args...]
       scripts/run-witty-native-opengl.sh [--print-command] --wittyrc-effective [window args...]
       scripts/run-witty-native-opengl.sh [--print-command] --window-config-template
       scripts/run-witty-native-opengl.sh [--print-command] --window-config-default-path
       scripts/run-witty-native-opengl.sh [--print-command] --window-config-init [config args...]
       scripts/run-witty-native-opengl.sh [--print-command] --window-config-check [config args...]
       scripts/run-witty-native-opengl.sh [--print-command] --window-config-effective [window args...]
       scripts/run-witty-native-opengl.sh [--print-command] --renderer-backend-info
       scripts/run-witty-native-opengl.sh [--print-command] --keyboard-protocol-diagnostics
       scripts/run-witty-native-opengl.sh [--print-command] --keyboard-protocol-capture
       scripts/run-witty-native-opengl.sh [--print-command] --keyboard-protocol-live-compare
       scripts/run-witty-native-opengl.sh [--print-command] --keyboard-protocol-live-compare-list
       scripts/run-witty-native-opengl.sh [--print-command] --keyboard-protocol-native-diagnostics

Runs Witty native window mode with WGPU_BACKEND=gl.
Helper modes reuse the same binary selection and do not append --window. Most
helpers are non-window commands; --keyboard-protocol-native-diagnostics opens
its own minimal diagnostic window.

Options:
  --print-command  Print the selected command instead of launching it.
  -h, --help       Show this help text.

Environment:
  WITTY_NATIVE_BIN=/path/to/witty  Use an installed or custom binary.
MSG
}

print_command() {
  printf 'WGPU_BACKEND=gl'
  for part in "$@"; do
    printf ' %q' "${part}"
  done
  printf '\n'
}

print_null_args() {
  if [[ $# -gt 0 ]]; then
    printf '%s\0' "$@"
  fi
}

witty_args() {
  case "${1:-}" in
    --font-list)
      shift
      if [[ $# -gt 0 && "${1}" != --* ]]; then
        printf '%s\0' --font-list --font-list-filter "$1"
        shift
      else
        printf '%s\0' --font-list
      fi
      print_null_args "$@"
      ;;
    --window-config-template | --window-config-default-path | --window-config-init | \
      --wittyrc-template | --wittyrc-default-path | --wittyrc-init | \
      --wittyrc-check | --wittyrc-effective | \
      --window-config-check | --window-config-effective | --renderer-backend-info | \
      --renderer-no-surface-diagnostics | --keyboard-protocol-diagnostics | \
      --keyboard-protocol-capture | --keyboard-protocol-live-compare | \
      --keyboard-protocol-live-compare-list | --keyboard-protocol-live-compare-case | \
      --keyboard-protocol-live-compare-output | \
      --keyboard-protocol-native-diagnostics)
      print_null_args "$@"
      ;;
    *)
      printf '%s\0' --window "$@"
      ;;
  esac
}

run_selected() {
  local -a app_args=("$@")
  if [[ -n "${WITTY_NATIVE_BIN}" ]]; then
    if [[ "${PRINT_COMMAND}" == "1" ]]; then
      print_command "${WITTY_NATIVE_BIN}" "${app_args[@]}"
      exit 0
    fi
    exec env WGPU_BACKEND=gl "${WITTY_NATIVE_BIN}" "${app_args[@]}"
  fi

  if [[ -x "${DEBUG_BIN}" ]]; then
    if [[ "${PRINT_COMMAND}" == "1" ]]; then
      print_command "${DEBUG_BIN}" "${app_args[@]}"
      exit 0
    fi
    exec env WGPU_BACKEND=gl "${DEBUG_BIN}" "${app_args[@]}"
  fi

  if [[ "${PRINT_COMMAND}" == "1" ]]; then
    print_command cargo run -p witty-app -- "${app_args[@]}"
    exit 0
  fi

  cd "${ROOT_DIR}"
  exec env WGPU_BACKEND=gl cargo run -p witty-app -- "${app_args[@]}"
}

while [[ $# -gt 0 ]]; do
  case "${1}" in
    --print-command)
      PRINT_COMMAND=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      break
      ;;
  esac
done

mapfile -d '' -t APP_ARGS < <(witty_args "$@")
run_selected "${APP_ARGS[@]}"
