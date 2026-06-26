#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEFAULT_OUTPUT_DIR="${ROOT_DIR}/target/keyboard-protocol-live-compare"
OUTPUT_DIR="${WITTY_KEYBOARD_PROTOCOL_OUTPUT_DIR:-${DEFAULT_OUTPUT_DIR}}"
WITTY_BIN="${WITTY_NATIVE_BIN:-${ROOT_DIR}/target/debug/witty}"
if [[ -n "${WITTY_NATIVE_BIN:-}" ]]; then
  BUILD=0
else
  BUILD=1
fi
PRINT_PLAN=0
TERMINALS=()
CASES=()
ALL_TERMINALS=(kitty wezterm ghostty)

usage() {
  cat <<'MSG'
Usage: scripts/run-keyboard-protocol-live-compare-matrix.sh [options]

Launch Witty's keyboard protocol live compare helper inside installed terminal
emulators, saving each terminal's JSON report under target/.

Options:
  --terminal NAME     Terminal to include. Repeatable. Default: kitty, wezterm, ghostty.
                      Supported values: kitty, wezterm, ghostty.
  --case ID           Live compare case id to run. Repeatable. Default: all live cases.
  --output-dir PATH   Directory for per-terminal reports and runner scripts.
  --witty-bin PATH    Witty binary to execute. Default: target/debug/witty or WITTY_NATIVE_BIN.
  --build             Build target/debug/witty before launching. Default unless WITTY_NATIVE_BIN is set.
  --no-build          Do not build; require the selected Witty binary to exist.
  --print-plan        Print JSON describing detected terminals and launch commands without running.
  -h, --help          Show this help text.

Environment:
  WITTY_NATIVE_BIN                     Overrides the default Witty binary.
  WITTY_KEYBOARD_PROTOCOL_OUTPUT_DIR   Overrides the default output directory.
MSG
}

fail() {
  printf 'run-keyboard-protocol-live-compare-matrix: %s\n' "$*" >&2
  exit 1
}

absolute_path() {
  local path="$1"
  case "${path}" in
    /*) printf '%s\n' "${path}" ;;
    *) printf '%s/%s\n' "$(pwd)" "${path}" ;;
  esac
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

json_string_array() {
  local first=1
  local value
  printf '['
  for value in "$@"; do
    if [[ "${first}" == "0" ]]; then
      printf ', '
    fi
    json_string "${value}"
    first=0
  done
  printf ']'
}

print_shell_command() {
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

json_command_array() {
  json_string_array "$@"
}

supported_terminal() {
  case "$1" in
    kitty | wezterm | ghostty) return 0 ;;
    *) return 1 ;;
  esac
}

terminal_binary() {
  command -v "$1" 2>/dev/null || true
}

live_compare_args() {
  local report_path="$1"
  local case_id
  printf '%s\0' --keyboard-protocol-live-compare
  for case_id in "${CASES[@]}"; do
    printf '%s\0' --keyboard-protocol-live-compare-case "${case_id}"
  done
  printf '%s\0' --keyboard-protocol-live-compare-output "${report_path}"
}

runner_path_for() {
  local terminal="$1"
  printf '%s/%s-live-compare-runner.sh\n' "${OUTPUT_DIR}" "${terminal}"
}

report_path_for() {
  local terminal="$1"
  printf '%s/%s-live-compare.json\n' "${OUTPUT_DIR}" "${terminal}"
}

report_complete() {
  local report_path="$1"
  [[ -f "${report_path}" ]] && grep -q '"diagnostic": "keyboard-protocol-live-compare"' "${report_path}"
}

write_runner() {
  local terminal="$1"
  local report_path="$2"
  local runner_path="$3"
  local -a args
  mapfile -d '' -t args < <(live_compare_args "${report_path}")

  mkdir -p "$(dirname "${runner_path}")"
  {
    printf '#!/usr/bin/env bash\n'
    printf 'set -u\n'
    printf 'cd %q\n' "${ROOT_DIR}"
    printf 'printf %%s\\\\n %q > %q\n' '{"diagnostic":"keyboard-protocol-live-compare-pending"}' "${report_path}"
    printf 'env WGPU_BACKEND=gl %q' "${WITTY_BIN}"
    local arg
    for arg in "${args[@]}"; do
      printf ' %q' "${arg}"
    done
    printf '\n'
    printf 'status=$?\n'
    printf 'printf "\\nWitty keyboard live compare exited with status %%s.\\n" "${status}"\n'
    printf 'printf "Terminal: %%s\\n" %q\n' "${terminal}"
    printf 'printf "Report: %%s\\n" %q\n' "${report_path}"
    printf 'printf "Press Enter to close this window... "\n'
    printf 'IFS= read -r _\n'
    printf 'exit "${status}"\n'
  } >"${runner_path}"
  chmod +x "${runner_path}"
}

terminal_command() {
  local terminal="$1"
  local binary="$2"
  local runner_path="$3"
  local title="Witty keyboard live compare: ${terminal}"

  case "${terminal}" in
    kitty)
      printf '%s\0' "${binary}" --title "${title}" "${runner_path}"
      ;;
    wezterm)
      printf '%s\0' "${binary}" start --always-new-process --cwd "${ROOT_DIR}" -- "${runner_path}"
      ;;
    ghostty)
      printf '%s\0' "${binary}" "--title=${title}" "--working-directory=${ROOT_DIR}" -e "${runner_path}"
      ;;
    *)
      fail "unsupported terminal ${terminal}"
      ;;
  esac
}

summary_command() {
  printf '%s\0' "${WITTY_BIN}" --keyboard-protocol-live-compare-summary "${OUTPUT_DIR}"
}

terminal_plan_json() {
  local terminal="$1"
  local binary
  binary="$(terminal_binary "${terminal}")"
  local report_path
  report_path="$(report_path_for "${terminal}")"
  local runner_path
  runner_path="$(runner_path_for "${terminal}")"

  printf '    {\n'
  printf '      "name": %s,\n' "$(json_string "${terminal}")"
  if [[ -n "${binary}" ]]; then
    local -a command
    mapfile -d '' -t command < <(terminal_command "${terminal}" "${binary}" "${runner_path}")
    printf '      "available": true,\n'
    printf '      "binary": %s,\n' "$(json_string "${binary}")"
    printf '      "runnerPath": %s,\n' "$(json_string "${runner_path}")"
    printf '      "reportPath": %s,\n' "$(json_string "${report_path}")"
    printf '      "command": %s,\n' "$(json_command_array "${command[@]}")"
    printf '      "shellCommand": %s\n' "$(json_string "$(print_shell_command "${command[@]}")")"
  else
    printf '      "available": false,\n'
    printf '      "binary": null,\n'
    printf '      "runnerPath": %s,\n' "$(json_string "${runner_path}")"
    printf '      "reportPath": %s,\n' "$(json_string "${report_path}")"
    printf '      "command": [],\n'
    printf '      "shellCommand": null\n'
  fi
  printf '    }'
}

print_plan_json() {
  local -a selected=("$@")
  printf '{\n'
  printf '  "diagnostic": "keyboard-protocol-live-compare-matrix-plan",\n'
  printf '  "wittyBin": %s,\n' "$(json_string "${WITTY_BIN}")"
  printf '  "build": %s,\n' "$([[ "${BUILD}" == "1" ]] && printf true || printf false)"
  printf '  "outputDir": %s,\n' "$(json_string "${OUTPUT_DIR}")"
  printf '  "cases": %s,\n' "$(json_string_array "${CASES[@]}")"
  local -a summary
  mapfile -d '' -t summary < <(summary_command)
  printf '  "summaryCommand": %s,\n' "$(json_command_array "${summary[@]}")"
  printf '  "summaryShellCommand": %s,\n' "$(json_string "$(print_shell_command "${summary[@]}")")"
  printf '  "terminals": [\n'
  local index=0
  local terminal
  for terminal in "${selected[@]}"; do
    if [[ "${index}" -gt 0 ]]; then
      printf ',\n'
    fi
    terminal_plan_json "${terminal}"
    index=$((index + 1))
  done
  printf '\n  ]\n'
  printf '}\n'
}

result_json() {
  local terminal="$1"
  local status="$2"
  local exit_code="$3"
  local report_path="$4"
  local runner_path="$5"
  local binary="$6"
  local report_exists=false
  local report_is_complete=false
  if [[ -f "${report_path}" ]]; then
    report_exists=true
  fi
  if report_complete "${report_path}"; then
    report_is_complete=true
  fi

  printf '    {\n'
  printf '      "name": %s,\n' "$(json_string "${terminal}")"
  printf '      "status": %s,\n' "$(json_string "${status}")"
  printf '      "exitCode": %s,\n' "${exit_code}"
  if [[ -n "${binary}" ]]; then
    printf '      "binary": %s,\n' "$(json_string "${binary}")"
  else
    printf '      "binary": null,\n'
  fi
  printf '      "runnerPath": %s,\n' "$(json_string "${runner_path}")"
  printf '      "reportPath": %s,\n' "$(json_string "${report_path}")"
  printf '      "reportExists": %s,\n' "${report_exists}"
  printf '      "reportComplete": %s\n' "${report_is_complete}"
  printf '    }'
}

run_matrix() {
  local -a selected=("$@")
  mkdir -p "${OUTPUT_DIR}"
  local -a results=()
  local terminal

  for terminal in "${selected[@]}"; do
    local binary
    binary="$(terminal_binary "${terminal}")"
    local report_path
    report_path="$(report_path_for "${terminal}")"
    local runner_path
    runner_path="$(runner_path_for "${terminal}")"

    if [[ -z "${binary}" ]]; then
      printf 'Skipping %s: command not found\n' "${terminal}" >&2
      results+=("$(result_json "${terminal}" skipped 0 "${report_path}" "${runner_path}" "")")
      continue
    fi

    write_runner "${terminal}" "${report_path}" "${runner_path}"
    local -a command
    mapfile -d '' -t command < <(terminal_command "${terminal}" "${binary}" "${runner_path}")
    printf 'Launching %s: ' "${terminal}" >&2
    print_shell_command "${command[@]}" >&2
    printf '\n' >&2

    set +e
    "${command[@]}"
    local exit_code=$?
    set -e

    local status=failed
    if [[ "${exit_code}" -eq 0 ]] && report_complete "${report_path}"; then
      status=completed
    elif [[ "${exit_code}" -eq 0 ]]; then
      status=no-report
    fi
    results+=("$(result_json "${terminal}" "${status}" "${exit_code}" "${report_path}" "${runner_path}" "${binary}")")
  done

  printf '{\n'
  printf '  "diagnostic": "keyboard-protocol-live-compare-matrix",\n'
  printf '  "wittyBin": %s,\n' "$(json_string "${WITTY_BIN}")"
  printf '  "outputDir": %s,\n' "$(json_string "${OUTPUT_DIR}")"
  printf '  "cases": %s,\n' "$(json_string_array "${CASES[@]}")"
  local -a summary
  mapfile -d '' -t summary < <(summary_command)
  printf '  "summaryCommand": %s,\n' "$(json_command_array "${summary[@]}")"
  printf '  "summaryShellCommand": %s,\n' "$(json_string "$(print_shell_command "${summary[@]}")")"
  printf '  "results": [\n'
  local index
  for index in "${!results[@]}"; do
    if [[ "${index}" -gt 0 ]]; then
      printf ',\n'
    fi
    printf '%s' "${results[$index]}"
  done
  printf '\n  ]\n'
  printf '}\n'
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --terminal)
      [[ $# -ge 2 ]] || fail "--terminal requires a value"
      supported_terminal "$2" || fail "unsupported terminal $2"
      TERMINALS+=("$2")
      shift 2
      ;;
    --case)
      [[ $# -ge 2 ]] || fail "--case requires a case id"
      [[ -n "$2" ]] || fail "--case cannot be empty"
      CASES+=("$2")
      shift 2
      ;;
    --output-dir)
      [[ $# -ge 2 ]] || fail "--output-dir requires a path"
      [[ -n "$2" ]] || fail "--output-dir cannot be empty"
      OUTPUT_DIR="$(absolute_path "$2")"
      shift 2
      ;;
    --witty-bin)
      [[ $# -ge 2 ]] || fail "--witty-bin requires a path"
      [[ -n "$2" ]] || fail "--witty-bin cannot be empty"
      WITTY_BIN="$(absolute_path "$2")"
      BUILD=0
      shift 2
      ;;
    --build)
      BUILD=1
      shift
      ;;
    --no-build)
      BUILD=0
      shift
      ;;
    --print-plan | --dry-run)
      PRINT_PLAN=1
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

if [[ "${#TERMINALS[@]}" -eq 0 ]]; then
  TERMINALS=("${ALL_TERMINALS[@]}")
fi
OUTPUT_DIR="$(absolute_path "${OUTPUT_DIR}")"
WITTY_BIN="$(absolute_path "${WITTY_BIN}")"

if [[ "${PRINT_PLAN}" == "1" ]]; then
  print_plan_json "${TERMINALS[@]}"
  exit 0
fi

if [[ "${BUILD}" == "1" ]]; then
  (cd "${ROOT_DIR}" && cargo build -p witty-app)
fi
[[ -x "${WITTY_BIN}" ]] || fail "Witty binary is not executable: ${WITTY_BIN}"

run_matrix "${TERMINALS[@]}"
