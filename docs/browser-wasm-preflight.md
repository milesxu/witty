# Browser Wasm Preflight

Updated: 2026-05-29

## Goal

Prepare the native Rust/wgpu mainline for a browser-facing `witty-web` path without dragging native-only PTY, clipboard, or Wasmtime host dependencies into `wasm32-unknown-unknown`.

## Local Toolchain

Installed targets after this preflight:

| target | status | use |
| --- | --- | --- |
| `aarch64-unknown-linux-gnu` | installed | native development on this host |
| `wasm32-wasip2` | installed | Wasm component guest fixture builds |
| `wasm32-unknown-unknown` | installed during this task | browser/WebGPU build target |

## Wasm Check Results

| crate | command | result | implication |
| --- | --- | --- | --- |
| `witty-core` | `cargo check -p witty-core --target wasm32-unknown-unknown` | pass | `vte` + grid model can be reused in browser. |
| `witty-plugin-api` | `cargo check -p witty-plugin-api --target wasm32-unknown-unknown` | pass | Manifest/events/permissions types can be shared with browser. |
| `witty-render-wgpu` | `cargo check -p witty-render-wgpu --target wasm32-unknown-unknown` | pass | Frame planning plus current `wgpu`/`glyphon` renderer code is at least compile-compatible with browser target. Runtime surface creation still needs browser-specific entry code. |
| `witty-transport` | `cargo check -p witty-transport --target wasm32-unknown-unknown` | fail | `portable-pty` pulls native serial/file descriptor dependencies. Keep local PTY behind native cfg or move it to a native-only crate/module. |
| `witty-plugin-wasm` | `cargo check -p witty-plugin-wasm --target wasm32-unknown-unknown` | fail | Wasmtime host runtime requires virtual memory/mmap support and is native/server-side only. Browser plugins need a separate browser runtime plan. |
| `witty-ui` | `cargo check -p witty-ui --target wasm32-unknown-unknown` | fail | Currently depends on `witty-transport` and `witty-plugin-wasm`, so browser UI composition needs feature-gated transport/plugin runtime boundaries. |
| `witty-app` | `cargo check -p witty-app --target wasm32-unknown-unknown` | fail | Native binary intentionally depends on `winit`, `arboard`, `LocalPtyTransport`, filesystem plugin discovery, and blocking smoke helpers. Do not make this crate the browser entry point. |

## Main Blockers

1. `witty-transport` exports `LocalPtyTransport` unconditionally and depends on `portable-pty` unconditionally.
2. `witty-ui` depends on `witty-plugin-wasm` unconditionally even though the browser path should start with built-in commands and mock/gateway transport.
3. `witty-app` mixes reusable app behavior with native-only adapters:
   - `winit` desktop shell,
   - `arboard` clipboard/primary selection,
   - `LocalPtyTransport`,
   - filesystem plugin discovery,
   - blocking `std::thread::sleep` smoke loops.
4. `WgpuRectRenderer::new` is currently native-window oriented. Browser startup should instantiate it from a canvas/surface path, or add a small browser-specific renderer constructor while reusing `FramePlanner`, `RetainedFramePlanner`, and batch data structures.

## Smallest Browser Path

1. Keep these crates shared:
   - `witty-core`
   - `witty-plugin-api`
   - `witty-render-wgpu`
2. Split transport by target:
   - keep `MockTransport` target-independent,
   - gate `LocalPtyTransport` and `portable-pty` with `cfg(not(target_arch = "wasm32"))`,
   - later add `BrowserGatewayTransport` using WebSocket/web-sys.
3. Split plugin host by target:
   - keep `BuiltInPlugin` and command registry usable without Wasmtime,
   - make Wasmtime plugin install support a native feature or native-only dependency,
   - postpone browser plugin execution until there is a browser runtime decision.
4. Add `crates/witty-web` as the first browser crate:
   - crate type `cdylib` + `rlib`,
   - dependencies on `wasm-bindgen`, `web-sys`, `console_error_panic_hook` later,
   - start with mock terminal replay into `FramePlanner`/`RetainedFramePlanner`,
   - expose a smokeable pure Rust function first, then bind it to canvas/WebGPU.

## Recommended Next Task

`m46-witty-web-skeleton`

Add a minimal `witty-web` crate to the workspace that compiles for `wasm32-unknown-unknown` and proves browser-side reuse of `witty-core` plus `witty-render-wgpu` frame planning. Do not wire a real canvas yet; first make the browser crate own a mock replay smoke so target checks stay deterministic.
