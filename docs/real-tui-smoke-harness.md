# Real TUI Smoke Harness

Updated: 2026-05-30

m143 added the first reusable headless PTY runner for real terminal
applications. Later milestones extended it with real editor, tmux, and optional
vttest cases. It is intentionally separate from browser and GPU smokes so
terminal-core compatibility can be tested against real local programs quickly.

## CLI

```text
witty --real-tui-smoke <case-id>
```

Use `witty --real-tui-smoke list` to print the implemented case ids as a
JSON array without running a smoke case.

Use `witty --real-tui-smoke all` to run each implemented case in registry
order, write the usual per-case artifacts, and write a suite summary to
`target/real-tui-smoke/all.json`. The command exits successfully when cases
pass or are skipped because optional tools are missing, and exits with failure
when any case fails.

Implemented cases:

| Case | Status |
| --- | --- |
| `less-basic-restore` | implemented |
| `vim-basic-edit` | implemented |
| `nvim-basic-edit` | implemented, skipped if `nvim` is missing |
| `tmux-basic-pane` | implemented, skipped if `tmux` is missing |
| `htop-or-btop-redraw` | implemented, skipped if both tools are missing |
| `vttest-subset` | implemented, skipped if `vttest` is missing |

Unknown case ids fail with the available case list. `list` and `all` are CLI
helpers, not smoke case ids, so they are not included in the case list JSON.

## Harness Behavior

The runner:

- spawns a real PTY at `24x80`.
- runs with a 10 second deadline.
- feeds all PTY output through `BasicTerminal`.
- drains host actions after every output burst.
- writes terminal reply host actions back through the PTY input boundary.
- counts OSC 52 clipboard actions without storing payload text.
- writes a bounded JSON report to `target/real-tui-smoke/<case>.json`.
- writes a bounded suite report to `target/real-tui-smoke/all.json` when using
  `--real-tui-smoke all`.
- writes raw PTY output only when `WITTY_TUI_SMOKE_CAPTURE_RAW=1`.

The artifact directory can be overridden with
`WITTY_TUI_SMOKE_ARTIFACT_DIR`.

## `less-basic-restore`

The case creates a deterministic 120-line fixture and runs:

```text
less -R -M <fixture>
```

with controlled environment values:

```text
TERM=xterm-256color
COLORTERM=truecolor
LC_ALL=C.UTF-8
LESS=
HOME=<temporary smoke home>
```

Assertions:

- initial pager text includes `Line 001`.
- search for `Line 050` makes that line visible.
- `q` exits the process before the deadline.
- process exit status is `0`.
- final main-screen snapshot does not retain alternate-screen pager content.

If `less` is missing, the runner writes an explicit `skipped` report instead of
silently passing.

## `vim-basic-edit`

The case creates a deterministic text fixture and runs:

```text
vim -Nu NONE -n -i NONE -N <fixture>
```

with isolated `HOME`, `XDG_*`, and empty `VIMINIT`/`GVIMINIT`/`EXINIT`
environment values.

Assertions:

- editor startup shows fixture text, file name, or a recognizable status line.
- `gg0i...<Esc>` makes the inserted smoke token visible.
- `:wq` exits before the deadline.
- process exit status is `0`.
- saved file begins with the inserted token and still contains fixture text.
- final main-screen snapshot does not retain alternate-screen editor content.

If `vim` is missing, the runner writes an explicit `skipped` report.

## `nvim-basic-edit`

The optional Neovim case uses the same fixture, input script, and assertions as
`vim-basic-edit`, but runs:

```text
nvim --clean -n <fixture>
```

If `nvim` is missing, the runner writes an explicit `skipped` report.

## `tmux-basic-pane`

The tmux case uses an isolated socket path and temporary config:

```text
tmux -S <temp-socket> -f <temp-config> -u new-session ...
```

The config enables `set-clipboard on`, status line rendering, and a deterministic
`Witty` status-left marker without reading user tmux configuration.

Assertions:

- initial attached session shows `TMUX READY`.
- prefix split-pane creates a second pane and active pane output shows
  `TMUX PANE OK`.
- an OSC 52 write emitted from inside tmux is forwarded as a terminal host
  action.
- OSC 52 payload text is not rendered into terminal cells.
- external `tmux list-panes` sees at least two panes on the isolated socket.
- prefix detach exits the tmux client with status `0`.
- final main-screen snapshot does not retain full-screen tmux content.

The runner attempts to kill only the isolated tmux server before removing the
temporary work directory.

## `htop-or-btop-redraw`

The optional process-viewer case prefers `htop` and falls back to `btop`.

Assertions:

- a recognizable process table or CPU/memory marker appears.
- at least two PTY output bursts are observed.
- `q` exits the selected tool before the deadline.
- process exit status is `0`.

The case uses isolated `HOME` and `XDG_*` directories so user configuration is
not read or mutated. See `htop-btop-redraw-smoke.md`.

If neither `htop` nor `btop` is installed, the runner writes an explicit
`skipped` report.

## `vttest-subset`

The optional vttest case runs:

```text
vttest 24x80.80
```

with the same controlled terminal environment as the other real-TUI cases. If
`WITTY_VTTEST_COMMANDS=<path>` is set, it instead runs:

```text
vttest -c <path> 24x80.80
```

The default path is a bounded startup/menu probe: wait for a recognizable
vttest marker, send `0`, and require a clean exit. This keeps the case visible
while local `vttest` is missing. Once the binary is installed, record a narrow
command replay file with `vttest -l` and use `WITTY_VTTEST_COMMANDS` to
exercise cursor movement, erase/alignment, SGR/color, and terminal-report pages.

See `vttest-subset-smoke.md` for the exact subset plan and replay policy.

If `vttest` is missing, the runner writes an explicit `skipped` report.

## Verification

Passed:

- `cargo fmt`
- `cargo test -p witty-app real_tui_smoke`
- `cargo test -p witty-app`
- `cargo clippy -p witty-app -- -D warnings`
- `target/debug/witty --real-tui-smoke less-basic-restore`
- `target/debug/witty --real-tui-smoke vim-basic-edit`
- `target/debug/witty --real-tui-smoke nvim-basic-edit`
- `target/debug/witty --real-tui-smoke tmux-basic-pane`
- `target/debug/witty --real-tui-smoke htop-or-btop-redraw` skipped locally
- `target/debug/witty --real-tui-smoke vttest-subset` skipped locally

Local run result:

```text
Real TUI smoke less-basic-restore status=passed artifact=target/real-tui-smoke/less-basic-restore.json
Real TUI smoke vim-basic-edit status=passed artifact=target/real-tui-smoke/vim-basic-edit.json
Real TUI smoke nvim-basic-edit status=passed artifact=target/real-tui-smoke/nvim-basic-edit.json
Real TUI smoke tmux-basic-pane status=passed artifact=target/real-tui-smoke/tmux-basic-pane.json
Real TUI smoke htop-or-btop-redraw status=skipped artifact=target/real-tui-smoke/htop-or-btop-redraw.json
Real TUI smoke vttest-subset status=skipped artifact=target/real-tui-smoke/vttest-subset.json
```

## Follow-Up

`m146-browser-real-tui-product-smoke` can now reuse the stable L1 cases through
`witty-gateway` and `witty --web`.
