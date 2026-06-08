# GUI Screenshot Regression Harness

`scripts/capture-gui-diagnostics.sh` captures a bounded GUI smoke screenshot with
the command palette and frame diagnostics overlay visible. It writes the image
and a JSON metadata sidecar for manual visual regression checks.

The harness launches `witty` with `--window-startup-report`, so stderr also
records the native OpenGL backend policy before `wgpu` adapter selection.

## Recommended Local Command

On this Wayland desktop, the reliable path is the current X11/Xwayland capture
route:

```bash
env WITTY_CAPTURE_MODE=current \
  WITTY_CAPTURE_PREFER_X11=1 \
  WAYLAND_DISPLAY= \
  XDG_SESSION_TYPE=x11 \
  scripts/capture-gui-diagnostics.sh target/gui-regression/diagnostics.xwd
```

This produces:

- `target/gui-regression/diagnostics.xwd`
- `target/gui-regression/diagnostics.xwd.metadata.json`
- `target/gui-regression/diagnostics.xwd.startup.log`

## Metadata

The metadata sidecar uses schema `witty.gui-screenshot.v1` and records:

- scenario name
- output paths
- startup log path
- byte size
- SHA-256
- `file` output
- capture mode, backend, and tool
- window title
- command used to launch `witty`
- relevant display and renderer environment variables

## Backend Notes

- `WITTY_CAPTURE_MODE=xvfb` is intended for Xvfb/X11-capable CI environments.
- `WITTY_CAPTURE_MODE=current` captures from the active desktop session.
- Native Wayland capture uses `grim` when available, but some compositors do not
  expose the `wlr-screencopy-unstable-v1` protocol. Force X11/Xwayland with
  `WITTY_CAPTURE_PREFER_X11=1` and `WAYLAND_DISPLAY=` on those desktops.
- On the Linux/M1000 host, prefer the default `WITTY_CAPTURE_MODE=xvfb`
  route for routine GUI screenshots. It sets `WGPU_BACKEND=gl`,
  `LIBGL_ALWAYS_SOFTWARE=1`, `LIBGL_DRI3_DISABLE=1`,
  `MESA_LOADER_DRIVER_OVERRIDE=llvmpipe`, `WINIT_UNIX_BACKEND=x11`, and does
  not touch Chromium or Vulkan.
- Current local result on 2026-06-01: Xvfb reached the OpenGL-only startup
  report but failed during `wgpu` GL surface creation before screenshot capture.
  See `native-opengl-xvfb-probe-result.md`.
