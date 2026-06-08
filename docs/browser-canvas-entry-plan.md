# Browser Canvas Entry Plan

Updated: 2026-05-29

## Goal

Add the first real browser canvas/WebGPU entry point without disturbing the native `winit` path. The next implementation should prove that a browser-owned `<canvas>` can be connected to the existing terminal state, frame planning, and wgpu renderer.

## Current State

- `witty-web` compiles for `wasm32-unknown-unknown`.
- `witty-web` already verifies:
  - `BasicTerminal` mock replay,
  - `RetainedFramePlanner`,
  - `TerminalApp<MockTransport>`,
  - built-in plugin command dispatch.
- `witty-render-wgpu` compiles for `wasm32-unknown-unknown`.
- Native-only blockers have been isolated from `witty-transport` and `witty-ui`.

## wgpu 29 Surface API Check

Local `wgpu 29.0.3` supports web canvas surface targets behind `cfg(web)`:

- `wgpu::SurfaceTarget::Canvas(web_sys::HtmlCanvasElement)`
- `wgpu::SurfaceTarget::OffscreenCanvas(web_sys::OffscreenCanvas)`

The existing native renderer constructor currently requires:

```rust
surface_target: impl Into<wgpu::SurfaceTarget<'static>>
    + wgpu::rwh::HasDisplayHandle
    + Debug
    + Send
    + Sync
    + Clone
    + 'static
```

That signature fits `winit::Window` but is too narrow for `SurfaceTarget::Canvas`, which does not need a native display handle. The first browser implementation should split surface initialization rather than force a fake display handle.

## Proposed Renderer Refactor

Keep `WgpuRectRenderer::new(window, width, height)` for native callers.

Add an internal constructor shaped like:

```rust
async fn from_surface_target(
    instance_descriptor: wgpu::InstanceDescriptor,
    surface_target: impl Into<wgpu::SurfaceTarget<'static>>,
    width: u32,
    height: u32,
) -> anyhow::Result<Self>
```

Native `new(...)` can keep using:

```rust
wgpu::InstanceDescriptor::new_with_display_handle(...)
```

Browser `new_for_canvas(...)` should use a default `InstanceDescriptor` and pass:

```rust
wgpu::SurfaceTarget::Canvas(canvas)
```

This keeps the adapter/device/surface-config/pipeline/glyphon setup shared.

## Proposed Browser Entry

In `witty-web`, add a wasm-only async entry:

```rust
#[wasm_bindgen]
pub async fn witty_start_canvas(canvas_id: String) -> Result<(), JsValue>
```

Implementation outline:

1. Install `console_error_panic_hook`.
2. Resolve `window.document.get_element_by_id(canvas_id)`.
3. Cast to `web_sys::HtmlCanvasElement`.
4. Read CSS/client size and device pixel ratio.
5. Set canvas backing width/height.
6. Initialize `WgpuRectRenderer::new_for_canvas(canvas, width, height).await`.
7. Build a `TerminalApp<MockTransport>` and feed a mock terminal snapshot.
8. Render the first frame.

Do not add real keyboard, clipboard, resize observer, WebSocket gateway, or animation loop in the first pass. Those should be separate tasks after a visible first frame exists.

## Dependency Additions

Add wasm-target dependencies only in `witty-web`:

| dependency | use |
| --- | --- |
| `console_error_panic_hook` | readable browser panic output |
| `wasm-bindgen-futures` | async `wgpu` init from wasm-bindgen |
| `web-sys` | `Window`, `Document`, `HtmlCanvasElement`, `Element` |

Keep these under `[target.'cfg(target_arch = "wasm32")'.dependencies]`.

## Verification Strategy

Immediate deterministic checks:

- `cargo fmt --all -- --check`
- `cargo test -p witty-web -- --nocapture`
- `cargo check -p witty-render-wgpu --target wasm32-unknown-unknown`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`

Browser runtime check should be a separate task after the entry compiles:

- build with `wasm-bindgen` or `trunk`,
- serve a tiny static HTML page,
- open it in Playwright/Chromium with WebGPU enabled,
- assert the canvas is nonblank via screenshot/pixel check.

## Non-Goals For The First Canvas Entry

- No real PTY in browser.
- No WebSocket gateway yet.
- No browser clipboard/IME yet.
- No Wasm plugin execution in browser.
- No persistent settings/profile UI.
- No attempt to run the native `witty-app` crate in the browser.

## Recommended Next Task

`m50-browser-canvas-entry-skeleton`

Add the wasm-only canvas entry and renderer constructor split, then stop at successful `wasm32-unknown-unknown` checks. Runtime browser automation should follow as `m51`.
