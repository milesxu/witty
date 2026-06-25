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

## User-Local Linux Install

Install Witty for daily GNOME use under the user-local prefix `~/.local`:

```text
scripts/install-witty-local.sh --dry-run
scripts/install-witty-local.sh
```

The installer builds the debug `witty-app` binary by default, installs it to
`~/.local/bin/witty`, installs hicolor icons as `dev.witty.Witty`, and writes
`~/.local/share/applications/dev.witty.Witty.desktop`. The installed launcher
uses `/usr/bin/env` to set `WGPU_BACKEND=gl` and `WITTY_LOG=...` before running
`~/.local/bin/witty --window`; it also sets `Icon=dev.witty.Witty`,
`Terminal=false`, and `StartupWMClass=dev.witty.Witty`. Native window logs are
written under
`$XDG_STATE_HOME/witty/logs/`, or `~/.local/state/witty/logs/` when
`XDG_STATE_HOME` is unset. Set `WITTY_LOG`, `RUST_LOG`, or the install-time
`WITTY_DESKTOP_LOG` override to change log levels; release builds keep the same
levelled logging path. The installer also writes an install marker at
`$XDG_STATE_HOME/witty/install-state.v1.json`, or
`~/.local/state/witty/install-state.v1.json` when `XDG_STATE_HOME` is unset.
Already running installed windows poll that marker and show an update notice
with a `Restart to update` button when a newer local install is detected.

The restart button writes a restart snapshot under the same Witty state
directory and starts the newly installed binary as
`witty --window --restore-state <snapshot>`. The snapshot stores window grid
size, tab metadata, launch program/args/cwd, safe environment metadata, and
profile-launched session metadata. It deliberately does not store terminal
text or claim ordinary local PTY child process continuity; shells and programs
are relaunched from the saved metadata. Use a terminal multiplexer or a future
persistent Witty daemon for lossless process continuity.

To validate without touching the real home directory:

```text
fake_home="$(mktemp -d)"
HOME="$fake_home" scripts/install-witty-local.sh --dry-run
HOME="$fake_home" scripts/install-witty-local.sh --no-build
desktop-file-validate "$fake_home/.local/share/applications/dev.witty.Witty.desktop"
```

After a real install, launch Witty once from the desktop entry, then pin the
running Witty window to the GNOME Shell dock. The desktop file id and native
Linux app id/window class both use `dev.witty.Witty` so GNOME can group the
pinned launcher with Witty windows.

## Local Shell And PTY

Native `--window` starts a local shell through the PTY transport. Use
`--program`, repeatable `--arg`, `--cwd`, and repeatable `--env KEY=VALUE` to
shape a single launch:

```text
cargo run -p witty-app -- --window --program /bin/zsh --arg -l
cargo run -p witty-app -- --window --cwd ~/src --program tmux --arg new-session
scripts/run-witty-native-opengl.sh --cwd ~/src/witty --env WITTY_SESSION=dev
```

`Ctrl+Shift+T` opens a New Session using the same launch defaults. `Ctrl+Tab`
opens the session switcher, advances the selected session, and switches when
Ctrl is released. Terminal OSC title updates replace the fallback title while
the child process runs. Restart restore reuses these launch defaults and
tab/profile metadata, but it does not preserve the live PTY process tree for
ordinary local shells.

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
cursor-shape = "block"
cursor-blink = true
cursor-blink-rate = "normal"
cursor-style-source = "program"
session-tab-position = "top"
session-tab-label = "index"
session-tab-show-single = false
session-tab-show-multiple = false
window-last-active-close = "close-window"
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
In window mode, an invalid `.wittyrc` is ignored instead of aborting startup;
Witty opens with defaults and prints a startup notice with the load error and
the `witty --wittyrc-check` validation command. The legacy native-window JSON
config still loads after `.wittyrc` and remains useful for window size, title,
launch command, cwd, env, scrollback, and other settings.

By default, exiting the last local shell, for example with Ctrl-D, closes the
Witty window/program. Set `window-last-active-close = "block"` in `.wittyrc`
or `window_last_active_close = "block"` in `window.v1.json` to keep Witty open
after the last shell exits. In that non-closing mode, Witty replaces the
exited terminal buffer with a compact empty-session screen that can start a
New Session or open the command palette for profile/plugin launch actions.

You can still set fonts through CLI flags, environment defaults, or
`window.v1.json`:

```text
scripts/run-witty-native-opengl.sh --font-list nerd
cargo run -p witty-app -- --window \
  --font-family "Maple Mono NF CN" \
  --font-size 16 \
  --terminal-padding 0 \
  --background-opacity 0.85 \
  --background-image /path/to/background.png \
  --background-image-fit cover \
  --background-overlay-color "#000000" \
  --background-overlay-opacity 0.20 \
  --cursor-shape bar \
  --cursor-blink true \
  --cursor-blink-rate slow \
  --cursor-style-source config \
  --font-path /path/to/SymbolsNerdFontMono-Regular.ttf
WITTY_FONT_FAMILY="Maple Mono NF CN" witty --window
```

`--font-list` is non-graphical; add a filter before copying the exact family
name into `--font-family`, `WITTY_FONT_FAMILY`, or config. `--font-path` is
repeatable and accepts `.ttf`, `.otf`, and `.ttc` files. `WITTY_FONT_PATHS`
uses the platform path-list separator, `:` on Linux. Terminal padding defaults
to `0`; set `terminal-padding = 8` in `.wittyrc` or pass `--terminal-padding 8`
to restore the earlier inset. Background opacity defaults to `1.0`; lower it
with `background-opacity = 0.85` or `--background-opacity 0.85`. Set
`background-image` to a path for an image-backed background, or to `"null"` to
fall back to desktop transparency. Background images default to
`background-image-fit = "cover"`, which scales the image to fill the window and
center-crops any overflow. The CLI spelling is `--background-image-fit cover`.
Set `background-overlay-color` and `background-overlay-opacity` to tint or dim
the desktop/image background behind terminal cells without remapping application
truecolor output; a black overlay around `0.15`-`0.30` is useful for busy
photographic backgrounds. CLI spellings are `--background-overlay-color #000000`
and `--background-overlay-opacity 0.25`.
Terminal colors can be themed from `.wittyrc`: `theme-foreground` and
`theme-background` set default SGR colors, `theme-cursor` sets the cursor
override or uses `"null"` for the renderer default, and `theme-palette` accepts
exactly 16 xterm-style ANSI colors. Direct truecolor output from applications is
not remapped by the theme, while indexed ANSI colors follow the configured
palette.
Cursor shape defaults to `block`; set `cursor-shape = "underline"` or
`"bar"` for horizontal or vertical cursors. Set `cursor-blink = false` for a
steady cursor. CLI spellings are `--cursor-shape block|underline|bar` and
`--cursor-blink true|false`. Cursor blink timing defaults to
`cursor-blink-rate = "normal"`; use `"slow"` for a calmer fixed blink or
`"variable"` for a nonuniform cadence. The CLI spelling is
`--cursor-blink-rate normal|slow|variable`. Cursor style source defaults to
`cursor-style-source = "program"`, which lets terminal programs use DECSCUSR
to change cursor shape/blink. Use `"config"` to keep the configured
`cursor-shape` and `cursor-blink` visually fixed even inside full-screen TUIs.
The CLI spelling is `--cursor-style-source program|config`.

Witty supports a focused Kitty keyboard protocol / CSI-u subset for programs
such as Neovim. It tracks Kitty keyboard flags and supports
`DISAMBIGUATE_ESC_CODES` (`1`), `REPORT_EVENT_TYPES` (`2`),
`REPORT_ALTERNATE_KEYS` (`4`), `REPORT_ALL_KEYS_AS_ESC_CODES` (`8`), and
`REPORT_ASSOCIATED_TEXT` (`16`) across native and browser input paths. Flag
`1` disambiguates combinations such as `Ctrl-I` and keypad-vs-top-row keys
without changing legacy `Enter`, `Tab`, and `Backspace`; keypad reporting also
distinguishes NumLock-off navigation such as `KP_LEFT`. Flag `2` adds Kitty
press/repeat/release event sub-fields to CSI-u keys and functional-key escape
forms; flag `4` adds shifted and physical base-key sub-fields for character
keys; flag `8` also reports text keys and those legacy C0 keys plus left/right
modifier-key events as CSI-u. Witty also reports Kitty PUA functional key codes
for keys such as `F13`-`F35`, lock keys, media keys, volume keys, sided Hyper
keys, native sided Meta keys, and `AltGraph` when Kitty keyboard mode is active.
Flag `16` adds safe associated text codepoints when flag `8` is active. Kitty
graphics/image protocols are not part of this support. See
`docs/terminal-kitty-keyboard-protocol.md`.

The session tab strip is hidden by default so it never covers shell output or a
tmux status line. Set `session-tab-show-single = true` or
`session-tab-show-multiple = true` to render it, and use
`session-tab-position = "top"` or `"bottom"` to choose its row. The CLI
spellings are `--session-tab-show-single true`,
`--session-tab-show-multiple true`, `--session-tab-position top`, and
`--session-tab-label index`.

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
