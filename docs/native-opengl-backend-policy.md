# Native OpenGL Backend Policy

Updated: 2026-06-01

## Local Policy

On the Linux/M1000 development host, Witty native renderer work should use
the `wgpu` OpenGL backend only. Do not run Vulkan-backed renderer experiments or
local Playwright/Chromium WebGPU smoke tests on this machine until the GPU driver
stack is proven stable.

Browser/WebGPU and Vulkan backend work can continue on other machines with
stable driver support.

## aibookmx Warp Reference

Read-only inspection of `aibookmx` confirmed a useful precedent:

- System Warp entry: `/usr/bin/warp-terminal` is a symlink to
  `/opt/warpdotdev/warp-terminal/warp`.
- User OSS Warp entry:
  `/home/xuming/.local/opt/warpdotdev/warp-terminal-oss/warp-oss`.
- User desktop launchers include OpenGL-specific entries:
  - `dev.warp.Warp.OpenGL.desktop`
  - `dev.warp.WarpOss.OpenGL.desktop`
- Those launchers use `Exec=env WGPU_BACKEND=gl ...`, making OpenGL selection a
  launcher-level policy instead of relying on users to export an environment
  variable manually.
- Warp source checkout observed on `aibookmx`: branch `master`, commit
  `d0f045c01bacbd845a631d07da30f277cfd2b98d`.

Relevant Warp design points:

- Warp keeps `wgpu` backend selection explicit via `WGPU_BACKEND`.
- Warp has a `GraphicsBackend` enum with labels for Vulkan, OpenGL, Metal,
  DirectX 12, and browser WebGPU.
- Warp has crash-recovery mechanisms such as `force-x11`,
  `force-dedicated-gpu`, `disable-opengl`, and `force-vulkan`.
- Warp's Cargo configuration disables the browser WebGPU feature while enabling
  native backend features such as `gles`, `vulkan`, `metal`, and `dx12`.

## Witty Decision

Witty now pins Linux native `WgpuRectRenderer::new` to
`wgpu::Backends::GL`. Non-Linux native platforms keep the existing platform
default backend selection. The browser/wasm path remains in the tree, but it is
not the local priority path on the Linux/M1000 machine.

The native policy can be checked without touching the display stack:

```text
cargo run -p witty-app -- --renderer-backend-info
cargo run -p witty-app -- --renderer-no-surface-diagnostics
```

The first command prints JSON with `opens_window=false` and
`enumerates_adapter=false`. The second adds representative frame-planner stats
while still avoiding window, surface, adapter, and device creation. These are
policy/CPU diagnostics, not GPU probes.

When a real native window startup probe is needed, use:

```text
cargo run -p witty-app -- --window --window-startup-report --window-exit-after-ms 1200
```

The startup report is emitted after native window creation and before `wgpu`
renderer initialization. It names the native backend policy, surface size, and
`vulkan_enabled_by_witty=false`. Renderer initialization failures include
the same backend policy fields in stderr.

See `native-opengl-window-startup.md` for the report fields and safety boundary.

Follow-up work should favor:

- bounded native window startup probes that remain OpenGL-only on this host
- the checked-in Linux desktop launcher template in
  `packaging/linux/dev.witty.Witty.OpenGL.desktop`, which makes
  OpenGL backend selection explicit with `Exec=env WGPU_BACKEND=gl ...`
- non-browser CLI smoke tests for renderer/frame-planner correctness
- deferring Chromium/WebGPU runtime smoke tests to a safer platform
