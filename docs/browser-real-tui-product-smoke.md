# Browser Real TUI Product Smoke

Updated: 2026-06-01

m146 added a focused browser/product smoke for real terminal applications, and
m149 extends it to editor and multiplexer cases. It is separate from the broad
browser feature smoke so failures in real PTY programs are easier to diagnose.

## CLI

```text
WITTY_WEB_SMOKE_GATEWAY=rust WITTY_WEB_REAL_TUI_CASE=less-basic-restore scripts/run-witty-web-real-tui-smoke.sh
WITTY_WEB_SMOKE_GATEWAY=launcher WITTY_WEB_REAL_TUI_CASE=less-basic-restore scripts/run-witty-web-real-tui-smoke.sh
WITTY_WEB_SMOKE_GATEWAY=rust WITTY_WEB_REAL_TUI_CASE=vim-basic-edit scripts/run-witty-web-real-tui-smoke.sh
WITTY_WEB_SMOKE_GATEWAY=launcher WITTY_WEB_REAL_TUI_CASE=tmux-basic-pane scripts/run-witty-web-real-tui-smoke.sh
```

Supported cases:

| Case | Gateway Modes | Purpose |
| --- | --- | --- |
| `less-basic-restore` | `rust`, `launcher` | real pager output, search input, clean quit, nonblank browser canvas |
| `vim-basic-edit` | `rust`, `launcher` | real editor startup, insert/write/quit path, nonblank browser canvas, saved-file assertion |
| `tmux-basic-pane` | `rust`, `launcher` | real multiplexer startup, split pane input, detach/exit path, nonblank browser canvas |

The runner builds the web smoke assets, starts Chromium through Playwright,
starts either `witty-gateway` or product `witty --web`, and launches real
`less` inside a controlled shell environment:

```text
TERM=xterm-256color
COLORTERM=truecolor
LC_ALL=C.UTF-8
LESS=
```

Editor and tmux cases additionally run with isolated `HOME` and `XDG_*`
directories under `target/witty-web-real-tui/` so local user configuration is not
read or mutated.

On the Linux/M1000 development host, `.witty-local-opengl-only` disables
this runner before asset builds or Chromium startup unless
`WITTY_ALLOW_LOCAL_CHROMIUM_SMOKE=1` is set deliberately. Browser real-TUI
coverage should run on a safer platform until the local display driver stack is
stable.

## Browser Input Boundary

The browser app exposes a narrow smoke helper:

```text
window.wittySendGatewayInputBytes(bytes)
window.wittyReadScreenText()
```

`wittySendGatewayInputBytes(bytes)` sends a raw gateway `input` frame
directly over the active WebSocket. `wittyReadScreenText()` returns the last
screen-text snapshot cached from the serialized gateway processing path instead
of calling into wasm from Playwright while real application output is still
arriving. These helpers are used only by the real-TUI browser smoke; the normal
keyboard path remains covered by `scripts/run-witty-web-smoke.sh`.

## Assertions

The `less-basic-restore` browser smoke verifies:

- gateway ready frame arrives.
- real `less` output includes `Line 001`.
- sending `/Line 050\r` through the browser WebSocket reaches the PTY and
  produces output containing `Line 050`.
- sending `q` exits the gateway process cleanly.
- Chromium canvas screenshot is nonblank.

The `vim-basic-edit` browser smoke verifies:

- real `vim` output includes the deterministic fixture.
- sending `gg0i...<Esc>` through the browser WebSocket produces the inserted
  token in gateway output.
- a nonblank Chromium canvas screenshot is captured while the editor is active.
- sending `:wq\r` exits the gateway process cleanly.
- the edited fixture file starts with the inserted token and still contains the
  original text.

The `tmux-basic-pane` browser smoke verifies:

- real tmux output includes `TMUX READY`.
- browser WebSocket input sends prefix split-pane, disables echo in the active
  pane, and prints `TMUX BROWSER PANE OK`.
- a nonblank Chromium canvas screenshot is captured while tmux is active.
- prefix detach exits the gateway process cleanly.
- the isolated tmux server is killed during cleanup.

Screenshots are written under:

```text
target/witty-web-real-tui/
```

## Verification

Historical passing coverage before the Linux/M1000 local guard:

```text
node --check scripts/run-witty-web-real-tui-smoke.mjs
node --check crates/witty-web/static/app.js
bash -n scripts/run-witty-web-real-tui-smoke.sh
cargo test -p witty-web
WITTY_WEB_SMOKE_GATEWAY=rust WITTY_WEB_REAL_TUI_CASE=less-basic-restore scripts/run-witty-web-real-tui-smoke.sh
WITTY_WEB_SMOKE_GATEWAY=launcher WITTY_WEB_REAL_TUI_CASE=less-basic-restore scripts/run-witty-web-real-tui-smoke.sh
WITTY_WEB_SMOKE_GATEWAY=rust WITTY_WEB_REAL_TUI_CASE=vim-basic-edit scripts/run-witty-web-real-tui-smoke.sh
WITTY_WEB_SMOKE_GATEWAY=launcher WITTY_WEB_REAL_TUI_CASE=tmux-basic-pane scripts/run-witty-web-real-tui-smoke.sh
scripts/run-witty-web-smoke.sh
```

Current safe local verification on the guarded host:

```text
node --check scripts/run-witty-web-real-tui-smoke.mjs
bash -n scripts/run-witty-web-real-tui-smoke.sh
scripts/run-witty-web-real-tui-smoke.sh
node scripts/run-witty-web-real-tui-smoke.mjs
```

The final two commands are expected to exit at the local guard before launching
Chromium.

Historical local real-TUI browser outputs:

```text
Witty browser real-TUI smoke less-basic-restore via rust passed; gateway exit=0; screenshot=/home/mingxu/src/witty/target/witty-web-real-tui/less-basic-restore-rust.png
Witty browser real-TUI smoke less-basic-restore via launcher passed; gateway exit=0; screenshot=/home/mingxu/src/witty/target/witty-web-real-tui/less-basic-restore-launcher.png
Witty browser real-TUI smoke vim-basic-edit via rust passed; gateway exit=0; screenshot=/home/mingxu/src/witty/target/witty-web-real-tui/vim-basic-edit-rust.png
Witty browser real-TUI smoke tmux-basic-pane via launcher passed; gateway exit=0; screenshot=/home/mingxu/src/witty/target/witty-web-real-tui/tmux-basic-pane-launcher.png
```
