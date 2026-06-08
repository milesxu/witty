#!/usr/bin/env bash
set -euo pipefail

mode="${WITTY_CAPTURE_MODE:-xvfb}"
if [[ $# -gt 0 ]]; then
  out="$1"
elif [[ "$mode" == "current" && -n "${WAYLAND_DISPLAY:-}" && "${WITTY_CAPTURE_PREFER_X11:-0}" != "1" ]] && command -v grim >/dev/null; then
  out="target/gui-diagnostics.png"
else
  out="target/gui-diagnostics.xwd"
fi
mkdir -p "$(dirname "$out")"
out="$(realpath -m "$out")"
metadata_out="${WITTY_CAPTURE_METADATA_OUT:-$out.metadata.json}"
metadata_out="$(realpath -m "$metadata_out")"
mkdir -p "$(dirname "$metadata_out")"
startup_log="${WITTY_CAPTURE_STARTUP_LOG:-$out.startup.log}"
startup_log="$(realpath -m "$startup_log")"
mkdir -p "$(dirname "$startup_log")"
witty="$(realpath -m target/debug/witty)"
window_title="Witty Native OpenGL"
witty_args=(
  --window
  --window-startup-report
  --window-command-palette
  --window-diagnostics
  --window-exit-after-ms 2500
)

require_command() {
  local name="$1"
  if ! command -v "$name" >/dev/null; then
    echo "$name is required" >&2
    exit 127
  fi
}

json_escape() {
  local value="$1"
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  value="${value//$'\n'/\\n}"
  value="${value//$'\r'/\\r}"
  value="${value//$'\t'/\\t}"
  printf '%s' "$value"
}

metadata_value() {
  local value="$1"
  printf '"%s"' "$(json_escape "$value")"
}

write_metadata() {
  local backend="$1"
  local tool="$2"
  local bytes="$3"
  local sha256="$4"
  local file_info="$5"
  local created_utc="$6"
  local command_display
  command_display="$witty ${witty_args[*]}"

  {
    printf '{\n'
    printf '  "schema": "witty.gui-screenshot.v1",\n'
    printf '  "created_utc": %s,\n' "$(metadata_value "$created_utc")"
    printf '  "scenario": "diagnostics-command-palette",\n'
    printf '  "output": %s,\n' "$(metadata_value "$out")"
    printf '  "metadata": %s,\n' "$(metadata_value "$metadata_out")"
    printf '  "startup_log": %s,\n' "$(metadata_value "$startup_log")"
    printf '  "bytes": %s,\n' "$bytes"
    printf '  "sha256": %s,\n' "$(metadata_value "$sha256")"
    printf '  "file": %s,\n' "$(metadata_value "$file_info")"
    printf '  "mode": %s,\n' "$(metadata_value "$mode")"
    printf '  "backend": %s,\n' "$(metadata_value "$backend")"
    printf '  "tool": %s,\n' "$(metadata_value "$tool")"
    printf '  "window_title": %s,\n' "$(metadata_value "$window_title")"
    printf '  "witty": %s,\n' "$(metadata_value "$witty")"
    printf '  "command": %s,\n' "$(metadata_value "$command_display")"
    printf '  "cwd": %s,\n' "$(metadata_value "$PWD")"
    printf '  "env": {\n'
    printf '    "DISPLAY": %s,\n' "$(metadata_value "${DISPLAY:-}")"
    printf '    "WAYLAND_DISPLAY": %s,\n' "$(metadata_value "${WAYLAND_DISPLAY:-}")"
    printf '    "XDG_SESSION_TYPE": %s,\n' "$(metadata_value "${XDG_SESSION_TYPE:-}")"
    printf '    "WITTY_CAPTURE_MODE": %s,\n' "$(metadata_value "$mode")"
    printf '    "WITTY_CAPTURE_PREFER_X11": %s,\n' "$(metadata_value "${WITTY_CAPTURE_PREFER_X11:-0}")"
    printf '    "WGPU_BACKEND": %s,\n' "$(metadata_value "${WGPU_BACKEND:-}")"
    printf '    "LIBGL_ALWAYS_SOFTWARE": %s,\n' "$(metadata_value "${LIBGL_ALWAYS_SOFTWARE:-}")"
    printf '    "LIBGL_DRI3_DISABLE": %s,\n' "$(metadata_value "${LIBGL_DRI3_DISABLE:-}")"
    printf '    "MESA_LOADER_DRIVER_OVERRIDE": %s,\n' "$(metadata_value "${MESA_LOADER_DRIVER_OVERRIDE:-}")"
    printf '    "WINIT_UNIX_BACKEND": %s\n' "$(metadata_value "${WINIT_UNIX_BACKEND:-}")"
    printf '  }\n'
    printf '}\n'
  } > "$metadata_out"
}

cargo build -p witty-app

capture_x11='
set -euo pipefail
out="$1"
witty="$2"
startup_log="$3"
shift 3
"$witty" "$@" >"$startup_log" 2>&1 &
app_pid=$!

window_id=""
for _ in $(seq 1 50); do
  window_id="$(xdotool search --name "Witty Native OpenGL" 2>/dev/null | head -n 1 || true)"
  if [[ -n "$window_id" ]]; then
    break
  fi
  sleep 0.1
done

if [[ -z "$window_id" ]]; then
  echo "failed to find Witty window" >&2
  if [[ -s "$startup_log" ]]; then
    echo "Witty startup log:" >&2
    cat "$startup_log" >&2
  fi
  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  exit 1
fi

xwd -silent -id "$window_id" -out "$out"
wait "$app_pid"
'

capture_wayland='
set -euo pipefail
out="$1"
witty="$2"
startup_log="$3"
shift 3
delay="${WITTY_CAPTURE_DELAY:-1.5}"
"$witty" "$@" >"$startup_log" 2>&1 &
app_pid=$!

sleep "$delay"
if ! grim "$out"; then
  echo "failed to capture Wayland screenshot with grim" >&2
  if [[ -s "$startup_log" ]]; then
    echo "Witty startup log:" >&2
    cat "$startup_log" >&2
  fi
  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  exit 1
fi

wait "$app_pid"
'

case "$mode" in
  xvfb)
    require_command xvfb-run
    require_command xdotool
    require_command xwd
    capture_backend="xvfb-x11"
    capture_tool="xwd"
    WGPU_BACKEND="${WGPU_BACKEND:-gl}" \
    LIBGL_ALWAYS_SOFTWARE="${LIBGL_ALWAYS_SOFTWARE:-1}" \
    LIBGL_DRI3_DISABLE="${LIBGL_DRI3_DISABLE:-1}" \
    MESA_LOADER_DRIVER_OVERRIDE="${MESA_LOADER_DRIVER_OVERRIDE:-llvmpipe}" \
    WINIT_UNIX_BACKEND="${WINIT_UNIX_BACKEND:-x11}" \
    XDG_SESSION_TYPE=x11 \
    WAYLAND_DISPLAY= \
      xvfb-run -a bash -c "$capture_x11" bash "$out" "$witty" "$startup_log" "${witty_args[@]}"
    ;;
  current)
    if [[ -n "${WAYLAND_DISPLAY:-}" && "${WITTY_CAPTURE_PREFER_X11:-0}" != "1" ]] && command -v grim >/dev/null; then
      require_command grim
      capture_backend="current-wayland"
      capture_tool="grim"
      WGPU_BACKEND="${WGPU_BACKEND:-gl}" \
        bash -c "$capture_wayland" bash "$out" "$witty" "$startup_log" "${witty_args[@]}"
    else
      require_command xdotool
      require_command xwd
      capture_backend="current-x11"
      capture_tool="xwd"
      WGPU_BACKEND="${WGPU_BACKEND:-gl}" \
      WINIT_UNIX_BACKEND="${WINIT_UNIX_BACKEND:-x11}" \
        bash -c "$capture_x11" bash "$out" "$witty" "$startup_log" "${witty_args[@]}"
    fi
    ;;
  *)
    echo "unknown WITTY_CAPTURE_MODE: $mode" >&2
    exit 2
    ;;
esac

bytes="$(wc -c < "$out")"
sha256="$(sha256sum "$out" | awk '{print $1}')"
if command -v file >/dev/null; then
  file_info="$(file -b "$out")"
else
  file_info="unavailable"
fi
created_utc="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
write_metadata "$capture_backend" "$capture_tool" "$bytes" "$sha256" "$file_info" "$created_utc"

echo "GUI diagnostics screenshot: $out ($bytes bytes)"
echo "GUI diagnostics metadata: $metadata_out"
echo "GUI diagnostics sha256: $sha256"
