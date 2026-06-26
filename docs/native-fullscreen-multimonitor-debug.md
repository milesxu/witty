# Native Fullscreen Multi-Monitor Debug Notes

Updated: 2026-06-26

This note tracks the native fullscreen instability that appears only after an
external 4K monitor is attached. The current local observation is a single-screen
baseline: Witty has been running on the laptop's built-in display for hours
without a forced exit. Use this as the comparison point when the 4K display is
attached again.

## Known Symptom

When a 4K monitor is attached, previous manual runs showed this pattern:

- Witty enters fullscreen on one monitor.
- After focus moves to another monitor, the Witty window may shrink while Witty
  still believes it is fullscreen.
- Later F11 toggles may stop changing the window size.
- In later 4K runs, Witty sometimes exits and the crash wrapper shows
  `Witty could not start` with `run witty native window event loop: Exit Failure:1`.

The same kind of failure has not been observed in the current single-screen
session.

## Single-Screen Baseline

Observation time: `2026-06-26T16:02:51+08:00`.

Environment:

- session: `XDG_SESSION_TYPE=wayland`
- desktop: `XDG_CURRENT_DESKTOP=ubuntu:GNOME`
- display variables: `WAYLAND_DISPLAY=wayland-0`, `DISPLAY=:0`
- renderer backend: `WGPU_BACKEND=gl`
- monitor enumeration from `xrandr --listmonitors`:
  `XWAYLAND0 2880/300x1800/190+0+0`
- GNOME scaling: `scaling-factor=0`, `text-scaling-factor=1.0`

Observed Witty process:

- command: `/home/xuming/.local/bin/witty --window`
- observed PID: `15640`
- observed elapsed runtime: about `05:18:50`
- installed binary: `/home/xuming/.local/bin/witty`
- installed binary mtime: `2026-06-24 23:53:44 +0800`
- installed binary size: `454839112` bytes
- installed binary SHA-256:
  `834ffe7fb6f2f44140c0d44f9595e11720255af863bed27a1cefb79e86f948b3`
- repo commit at observation time:
  `068378a Fix witty-web wasm check`

Effective Witty configuration:

- native backend policy: `gl`
- `opengl_only=true`
- `vulkan_enabled_by_witty=false`
- font: `Maple Mono NF CN`, size `15`
- background opacity: `0.85`
- background image:
  `/home/xuming/Pictures/michael_pointner-agriculture-10314031.jpg`
- background image fit: `cover`
- overlay opacity: `0.45`
- cursor: underline, blink enabled, slow rate
- scrollback: `10000`
- session tab bar hidden for single and multiple sessions

## Log Baseline

Logs are written under:

```text
/home/xuming/.local/state/witty/logs/
```

The installed desktop entries launch Witty with:

```text
WGPU_BACKEND=gl
WITTY_LOG=info,witty_app::fullscreen=debug,wgpu=warn,naga=warn
```

Representative same-day fullscreen debug entries on the built-in display show a
stable fullscreen state:

- `app_fullscreen=true`
- `winit_fullscreen=true`
- `inner_size=2880x1800`
- `current_monitor=eDP-1`
- `current_monitor_size=2880x1800`
- `window_scale_factor=2.0`
- `current_monitor_scale_factor=2.0`
- `target_size=2880x1800`
- `target_scale_factor=2.0`
- `restore_size=1920x1080`
- `pending_restore=None`

Focus transitions on the single built-in display keep those values stable. Some
entries briefly report `current_monitor=None` during focus changes, but the
fullscreen inner size remains `2880x1800` and later monitor detection returns to
`eDP-1`.

This is the expected healthy state. In the 4K failure case, look for any point
where `app_fullscreen=true` and `winit_fullscreen=true` remain set while
`inner_size` drops below `target_size` or the current monitor changes
unexpectedly.

## 4K Reproduction Checklist

After attaching the 4K monitor, capture the baseline before reproducing:

```bash
date -Is
printf 'XDG_SESSION_TYPE=%s\nXDG_CURRENT_DESKTOP=%s\nDESKTOP_SESSION=%s\nWAYLAND_DISPLAY=%s\nDISPLAY=%s\nWGPU_BACKEND=%s\n' \
  "${XDG_SESSION_TYPE:-}" "${XDG_CURRENT_DESKTOP:-}" "${DESKTOP_SESSION:-}" \
  "${WAYLAND_DISPLAY:-}" "${DISPLAY:-}" "${WGPU_BACKEND:-}"
xrandr --listmonitors
gsettings get org.gnome.desktop.interface scaling-factor
gsettings get org.gnome.desktop.interface text-scaling-factor
scripts/run-witty-native-opengl.sh --renderer-backend-info
scripts/run-witty-native-opengl.sh --wittyrc-effective
scripts/run-witty-native-opengl.sh --window-config-effective
ps -eo pid,lstart,etime,etimes,stat,cmd --sort=start_time | rg 'witty --window|run-witty-native-opengl|cargo run -p witty-app'
```

If launching from a shell instead of the GNOME menu, use:

```bash
env WGPU_BACKEND=gl \
  WITTY_LOG='info,witty_app::fullscreen=debug,wgpu=warn,naga=warn' \
  WITTY_LOG_DIR="$HOME/.local/state/witty/logs" \
  /home/xuming/.local/bin/witty --window
```

After reproducing, save the relevant log slice:

```bash
log="$HOME/.local/state/witty/logs/witty.log.$(date +%F)"
rg -n 'fullscreen state|Exit Failure|could not start|ERROR|panic|calloop' "$log"
tail -n 200 "$log"
```

The most useful comparison fields are:

- `label`
- `app_fullscreen`
- `winit_fullscreen`
- `inner_size`
- `window_scale_factor`
- `current_monitor`
- `current_monitor_size`
- `current_monitor_scale_factor`
- `target_size`
- `target_scale_factor`
- `restore_size`
- `pending_restore`
- `reaffirm_active`

## Working Hypothesis

The current evidence points away from a general long-running Witty stability
problem. The issue is more likely tied to multi-monitor fullscreen state,
monitor/scale-factor transitions, or compositor focus behavior when an external
4K display is present.

Keep the next 4K run focused on comparing display topology and fullscreen debug
state against the single-screen baseline above before changing fullscreen code
again.
