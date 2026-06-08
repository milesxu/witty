# Witty

Witty is the Rust/wgpu mainline for a native-first terminal emulator. The
current daily path is a local Linux desktop window backed by `wgpu` and a local
PTY; browser and gateway work remains in the tree as an active but secondary
path.

The workspace is split into focused crates:

- `witty-app`: product binary, CLI, native window, launcher composition
- `witty-core`: terminal parser, state, snapshots, search, host actions
- `witty-transport`: local PTY, OpenSSH profile conversion, profile store types
- `witty-render-wgpu`: frame planning and `wgpu` renderer support
- `witty-ui`: shared terminal UI state, command palette, search, plugins
- `witty-plugin-api`: plugin manifests, permissions, and WIT ABI
- `witty-plugin-wasm`: native Wasmtime host for Wasm Component Model plugins
- `witty-gateway`, `witty-launcher`, `witty-web`: loopback browser path

## Daily Native Path

Use the OpenGL helper for local development on this Linux/M1000 machine:

```text
scripts/run-witty-native-opengl.sh
scripts/run-witty-native-opengl.sh --print-command
cargo run -p witty-app -- --window
```

`scripts/run-witty-native-opengl.sh` prefers `WITTY_NATIVE_BIN`, then
`target/debug/witty`, then `cargo run -p witty-app --`. It forces
`WGPU_BACKEND=gl` at the launcher boundary. Normal arguments are run as
`witty --window ...`; helper modes such as `--font-list`,
`--wittyrc-template`, `--wittyrc-check`, `--wittyrc-effective`,
`--window-config-template`, `--window-config-check`,
`--window-config-effective`, `--renderer-backend-info`, and
`--renderer-no-surface-diagnostics` are forwarded without opening a window.

Local policy is OpenGL-only. Do not run Vulkan renderer experiments or
Playwright/Chromium WebGPU smoke tests on this host. The marker
`.witty-local-opengl-only` makes the browser smoke scripts fail closed unless
`WITTY_ALLOW_LOCAL_CHROMIUM_SMOKE=1` is set deliberately. The desktop entry
template at `packaging/linux/dev.witty.Witty.OpenGL.desktop` uses:

```text
Exec=env WGPU_BACKEND=gl witty --window
```

Non-graphical policy checks:

```text
cargo run -p witty-app -- --renderer-backend-info
cargo run -p witty-app -- --renderer-no-surface-diagnostics
scripts/run-witty-native-opengl.sh --print-command --renderer-backend-info
```

For an approved bounded native window probe, keep it OpenGL-only:

```text
scripts/run-witty-native-opengl.sh --window-startup-report --window-exit-after-ms 1500
```

That probe reports the selected backend policy before renderer initialization
and emits first-frame font/frame metadata if the redraw succeeds.

## Local Shell And PTY

Native `--window` starts a local shell through the PTY transport. Use
`--program`, repeatable `--arg`, `--cwd`, and repeatable `--env KEY=VALUE` to
shape a single launch:

```text
cargo run -p witty-app -- --window --program /bin/zsh --arg -l
cargo run -p witty-app -- --window --cwd ~/src --program tmux --arg new-session
scripts/run-witty-native-opengl.sh --cwd ~/src/witty --env WITTY_SESSION=dev
```

`Ctrl+Shift+T` opens a new local tab using the same launch defaults. Terminal
OSC title updates replace the fallback title while the child process runs.

The browser launcher can also create a token-protected local gateway:

```text
scripts/build-witty-web-dist.sh
cargo run -p witty-app -- --web
cargo run -p witty-app -- --web --open-browser
```

On this host those browser paths are buildable/documented, but browser smoke is
deferred to machines where Chromium/WebGPU is safe.

## Fonts And Configuration

Witty's preferred developer config is `$HOME/.wittyrc`. It uses TOML and ships
with a bundled template:

```toml
font-family = "Maple Mono NF CN"
```

Create or inspect it without opening a window:

```text
scripts/run-witty-native-opengl.sh --wittyrc-template
scripts/run-witty-native-opengl.sh --wittyrc-default-path
scripts/run-witty-native-opengl.sh --wittyrc-init
scripts/run-witty-native-opengl.sh --wittyrc-check
scripts/run-witty-native-opengl.sh --wittyrc-effective
```

Use `--wittyrc <path>` for an explicit TOML file and `--no-wittyrc` to bypass
it. CLI flags and font environment variables take precedence over `.wittyrc`.
The legacy native-window JSON config still loads after `.wittyrc` and remains
useful for window size, title, launch command, cwd, env, scrollback, and other
settings.

You can still set fonts through CLI flags, environment defaults, or
`window.v1.json`:

```text
scripts/run-witty-native-opengl.sh --font-list nerd
cargo run -p witty-app -- --window \
  --font-family "Maple Mono NF CN" \
  --font-size 16 \
  --font-path /path/to/SymbolsNerdFontMono-Regular.ttf
WITTY_FONT_FAMILY="Maple Mono NF CN" witty --window
```

`--font-list` is non-graphical; add a filter before copying the exact family
name into `--font-family`, `WITTY_FONT_FAMILY`, or config. `--font-path` is
repeatable and accepts `.ttf`, `.otf`, and `.ttc` files. `WITTY_FONT_PATHS`
uses the platform path-list separator, `:` on Linux.

The compatible JSON config file is `window.v1.json` under the Witty config
directory, normally `$XDG_CONFIG_HOME/witty/` or `~/.config/witty/` on Linux.
Useful helpers:

```text
witty --window-config-default-path
witty --window-config-template
witty --window-config-init
witty --window-config-check
witty --window-config-effective
```

Use `--window-config <path>` or `WITTY_WINDOW_CONFIG` for an explicit file, and
`--no-window-config` to bypass JSON config loading. Precedence is:
CLI `--font-family` > `WITTY_FONT_FAMILY` > `.wittyrc` > `window.v1.json` >
built-in defaults.

## Plugins

Witty's plugin line is based on a manifest plus a Wasm Component Model ABI.
`witty-plugin-api` owns manifests, permissions, and WIT definitions.
`witty-plugin-wasm` is the native Wasmtime runtime. The host exposes narrow
imports such as host info and redacted profile summaries; sensitive terminal
output, clipboard payloads, local paths, SSH argv, and raw profile-store data
are not plugin-visible by default.

Native app and smoke paths accept plugin inputs such as `--wasm-plugin` and
`--plugin-dir`. Plugin actions that need host authority, including profile
picker or profile launch requests, are queued for trusted Witty UI to review
and resolve.

## SSH And Profiles

SSH support is modeled as profiles that Witty converts into OpenSSH-backed
`LocalPtyConfig` values. The native launcher and profile store own local file
I/O, profile selection, OpenSSH config import, and redacted summaries. Browser
profile picker flows receive only token-scoped, redacted data and never receive
the profile-store path, host secrets, raw OpenSSH argv, or private key data.

Important docs:

- `docs/ssh-profile-transport-plan.md`
- `docs/profile-store-file-plan.md`
- `docs/launcher-profile-picker-plan.md`
- `docs/profile-store-openssh-import-preview-plan.md`
- `docs/profile-store-openssh-import-confirmed-write-plan.md`
- `docs/plugin-runtime-selection.md`

## Browser And Backend Work

`witty-web`, `witty-gateway`, and `witty-launcher` remain active development
areas for a loopback browser UI, WebSocket gateway, packaged web assets, and
profile picker flows. This work is deliberately deferred from the local
acceptance path on the Linux/M1000 host because browser/WebGPU smoke can touch
Chromium and GPU driver paths. Keep local checks text-only or native OpenGL-only
unless a later task explicitly authorizes browser or GUI smoke.

Web assets resolve in this order: `--web-root`, `WITTY_WEB_ROOT`, installed
`share/witty/web` next to the executable, then `target/witty-web-dist`.

## Useful Checks

```text
cargo fmt --all --check
cargo check --workspace
cargo test -p witty-core
cargo test -p witty-transport
cargo test -p witty-render-wgpu
cargo test -p witty-ui
cargo test -p witty-plugin-api
cargo test -p witty-plugin-wasm
cargo test -p witty-app app_options
```

Do not stage or commit migration changes until the supervisor flow reaches the
verification and review tasks.
