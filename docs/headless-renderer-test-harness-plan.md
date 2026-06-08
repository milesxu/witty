# Headless Renderer Test Harness Plan

Updated: 2026-06-01

## Context

The Linux/M1000 host now uses native `wgpu` OpenGL-only policy, but the approved
Xvfb/software-GL screenshot probe failed before capture because `wgpu` could not
create a GL surface under Xvfb/EGL. Browser/Chromium/Vulkan paths remain
suspended locally.

This means local validation should not depend on surface creation. The renderer
harness needs layers with explicit driver contact boundaries.

## Harness Layers

| Layer | Runs on Linux/M1000 by default | Driver contact | Purpose |
| --- | --- | --- | --- |
| L0 frame-planner tests | yes | none | terminal snapshots to frame plans, overlays, row reuse, stats |
| L1 non-graphical policy checks | yes | none | `--renderer-backend-info`, backend drift detection |
| L2 startup-report parse/check | yes | no adapter request | validate report shape and OpenGL-only policy fields |
| L3 native window startup | approval only | window + adapter | bounded `--window-startup-report` probe |
| L4 Xvfb screenshot | blocked locally | Xvfb/EGL surface | screenshot regression if GL surface works |
| L5 offscreen `wgpu` texture render | safe hosts only | adapter + device | render pass correctness without window/surface |
| L6 browser WebGPU smoke | other platforms | Chromium/WebGPU | browser product validation |

## Proposed Implementation

1. Keep local CI-style checks on L0-L2:
   - `cargo test -p witty-render-wgpu`
   - `cargo test -p witty-app`
   - `cargo run -p witty-app -- --renderer-backend-info`
2. Add no-surface renderer unit coverage first:
   - construct representative `FramePlan` values
   - verify rect vertex planning, glyph batch budgeting, cache-stat helpers, and
     diagnostic JSON without creating a `wgpu::Instance`
3. Add an opt-in offscreen renderer harness only for safe hosts:
   - explicitly require an env flag such as `WITTY_ALLOW_WGPU_DEVICE_PROBE=1`
   - request `wgpu::Backends::GL` on Linux
   - request adapter with `compatible_surface=None`
   - render rect-only frames to an offscreen texture before adding glyphon text
   - write artifacts under `target/offscreen-renderer-smoke/`
4. Keep screenshot harness separate:
   - it remains useful where Xvfb/EGL works
   - on this host it is currently a documented blocked path

## Non-Goals

- Do not use Chromium as a local renderer validation fallback.
- Do not enable Vulkan on the Linux/M1000 host.
- Do not make adapter/device creation part of routine local validation.
- Do not retry Xvfb screenshots repeatedly without changing the GL/EGL setup.

## Next Candidate

`m322-renderer-no-surface-diagnostics` is complete:

```text
cargo run -p witty-app -- --renderer-no-surface-diagnostics
```

It serializes native backend policy plus representative frame-planner stats
without creating a window, surface, adapter, or device.
