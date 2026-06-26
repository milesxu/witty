# Witty Docs

These docs describe the current Rust/wgpu Witty mainline. Most files in this
directory are implementation notes from incremental terminal, renderer, plugin,
browser, and profile milestones. Treat this page and the top-level README as
the current orientation layer, then follow the linked plans for detail.

## Current Direction

Witty is native-first on the local Linux/M1000 development host:

- native window mode is the daily product path
- rendering uses `wgpu`
- local development forces the OpenGL backend with `WGPU_BACKEND=gl`
- local shell sessions run through the PTY transport
- browser/WebGPU smoke and Vulkan experiments are deferred from this host

Use:

```text
scripts/run-witty-native-opengl.sh
scripts/run-witty-native-opengl.sh --print-command --renderer-backend-info
cargo run -p witty-app -- --renderer-no-surface-diagnostics
```

The helper and desktop entry are documented in:

- `native-opengl-backend-policy.md`
- `native-opengl-window-startup.md`
- `native-fullscreen-multimonitor-debug.md`
- `linux-opengl-desktop-entry.md`
- `gui-screenshot-regression-harness.md`

## Fonts And Config

Witty's preferred developer config is `$HOME/.wittyrc`. It uses TOML and the
bundled starter template is:

```toml
font-family = "Maple Mono NF CN"
font-size = 14
terminal-padding = 0
background-opacity = 1.0
background-image = "null"
background-image-fit = "cover"
background-overlay-color = "#000000"
background-overlay-opacity = 0.0
theme-foreground = "#ffffff"
theme-background = "#000000"
theme-cursor = "null"
theme-palette = [
  "#000000", "#cd0000", "#00cd00", "#cdcd00",
  "#0000ee", "#cd00cd", "#00cdcd", "#e5e5e5",
  "#7f7f7f", "#ff0000", "#00ff00", "#ffff00",
  "#5c5cff", "#ff00ff", "#00ffff", "#ffffff",
]
```

Use the non-GUI helpers for setup and inspection:

```text
scripts/run-witty-native-opengl.sh --font-list nerd
scripts/run-witty-native-opengl.sh --wittyrc-template
scripts/run-witty-native-opengl.sh --wittyrc-init
scripts/run-witty-native-opengl.sh --wittyrc-check
scripts/run-witty-native-opengl.sh --wittyrc-effective
```

`--wittyrc <path>` selects an explicit TOML file and `--no-wittyrc` bypasses
it. CLI flags and `WITTY_FONT_FAMILY` / `WITTY_FONT_PATHS` override config
defaults. In window mode, invalid `.wittyrc` files are ignored with an in-window
startup notice instead of aborting startup. The existing `window.v1.json` under
the Witty config directory remains compatible and loads after `.wittyrc`, so it
can continue to provide window size, title, launch command, cwd, env,
scrollback, and related defaults.
Set `cursor-style-source = "config"` when Witty should keep the configured
cursor shape/blink even if a full-screen terminal program sends its own cursor
style escape sequences.
Use `theme-foreground`, `theme-background`, `theme-cursor`, and a 16-entry
`theme-palette` to theme default terminal colors and ANSI indexed colors from
`.wittyrc`; truecolor application output remains direct.
Use `background-overlay-color` and `background-overlay-opacity` to dim or tint
busy desktop/image backgrounds behind terminal cells without rewriting
application colors.

## PTY And Local Shell

Native `witty --window` starts a local PTY-backed shell by default. `--program`,
repeatable `--arg`, `--cwd`, and repeatable `--env KEY=VALUE` shape the process
for a launch. New local tabs inherit the launch defaults.

Related docs:

- `real-tui-smoke-harness.md`
- `terminal-shell-integration-osc133.md`
- `terminal-current-directory-osc7.md`
- `launcher-lifecycle-exit.md`

## Plugin System

Plugins use a manifest plus Wasm Component Model ABI. `witty-plugin-api` owns
the manifest, permissions, and WIT package. `witty-plugin-wasm` hosts native
Wasm plugins through Wasmtime. Host imports are deliberately narrow: plugin
code can request host-owned actions, but profile details, SSH argv, clipboard
payloads, and raw terminal-sensitive data stay out of the plugin ABI unless a
future permission explicitly changes that boundary.

Related docs:

- `plugin-runtime-selection.md`
- `wasmtime-runtime-spike.md`
- `plugin-host-info-import.md`
- `plugin-profile-store-summary-import.md`
- `plugin-profile-picker-request-action.md`
- `plugin-profile-launch-request-action.md`

## SSH And Profiles

SSH support is profile-driven. The transport layer models profiles and converts
launchable profiles into OpenSSH-backed `LocalPtyConfig` values. Trusted native
launcher/profile-store code owns local file I/O, OpenSSH config import, profile
selection, and redacted browser/profile-picker summaries.

Related docs:

- `ssh-profile-transport-plan.md`
- `profile-store-file-plan.md`
- `profile-store-cli-plan.md`
- `launcher-profile-picker-plan.md`
- `profile-picker-import-entry-plan.md`
- `profile-store-openssh-import-preview-plan.md`
- `profile-store-openssh-import-confirmed-write-plan.md`

## Browser And Backend Work

Browser support is present but not the local daily acceptance path. The browser
line covers wasm rendering, loopback WebSocket gateway, web asset packaging,
profile picker/import flows, and Playwright smoke harnesses. On this machine,
`.witty-local-opengl-only` intentionally blocks browser smoke unless
`WITTY_ALLOW_LOCAL_CHROMIUM_SMOKE=1` is set for a deliberate run.

Related docs:

- `browser-wasm-preflight.md`
- `browser-gateway-websocket-plan.md`
- `browser-transport-boundary.md`
- `browser-runtime-smoke-harness.md`
- `web-asset-packaging-plan.md`
- `launcher-browser-smoke-hardening.md`

## Historical Notes

Some milestone files include dated command output, safe-host validation notes,
or paths from earlier worktrees. They are retained as planning history when the
context is explicit. Product-facing docs should use Witty, `/home/mingxu/src/witty`,
`git@github.com:milesxu/witty.git`, `witty-*` crates, and `WITTY_*` environment
names.
