# Native OpenGL Window Startup

Updated: 2026-06-01

`m318-native-opengl-window-startup-hardening` adds an explicit startup report
for bounded native window probes:

```text
cargo run -p witty-app -- --window --window-startup-report --window-exit-after-ms 1200
```

The report is written to stderr after the native window is created and before
`WgpuRectRenderer::new()` requests an adapter. It is intended for OpenGL-first
native debugging, not for browser/WebGPU validation.

The report includes:

| Field | Meaning |
| --- | --- |
| `native_backend_policy` | current native `wgpu` backend policy label |
| `opengl_only` | whether the native backend set is exactly OpenGL |
| `honors_wgpu_backend_env` | whether the policy reads `WGPU_BACKEND` |
| `surface_width`, `surface_height` | initial native window surface size |
| `will_request_adapter` | true because the next step initializes `wgpu` |
| `vulkan_enabled_by_witty` | always false for this path |
| `chromium` | false; this is not a browser smoke |

Renderer initialization failures also include the backend policy fields in
stderr so OpenGL adapter problems can be separated from accidental Vulkan or
Chromium paths.

On the Linux/M1000 development host, keep using the non-graphical
`--renderer-backend-info` check for routine validation. Run the window startup
probe only when a real native window test is worth touching the local display
stack.

For screenshot regression work, prefer the existing Xvfb harness:

```text
scripts/capture-gui-diagnostics.sh target/gui-regression/diagnostics.xwd
```

That harness now adds `--window-startup-report` automatically and defaults to
`WGPU_BACKEND=gl`, `LIBGL_ALWAYS_SOFTWARE=1`, `LIBGL_DRI3_DISABLE=1`,
`MESA_LOADER_DRIVER_OVERRIDE=llvmpipe`, and `WINIT_UNIX_BACKEND=x11`.

The first local run reached the OpenGL-only startup report but failed in Xvfb
surface creation. See `native-opengl-xvfb-probe-result.md`.
