# Real TUI Compatibility Smoke Plan

Updated: 2026-05-30

m141 defines the repeatable smoke strategy for real terminal applications after
OSC 52 clipboard support. The goal is to prove useful xterm-style behavior with
bounded scripts instead of relying only on parser unit tests and synthetic
browser gateway output.

## Current Inputs

Already available:

- native PTY transport through `LocalPtyTransport`.
- browser PTY path through `witty-gateway` and `witty --web`.
- native and browser key, mouse, focus, resize, clipboard, OSC 52, OSC 8,
  search, alternate-screen, and rendering smokes.
- `BasicTerminal::take_snapshot()` and browser `screen_text()` for deterministic
  text assertions.

Observed local tool availability on 2026-05-30:

| Tool | Local status | Smoke stance |
| --- | --- | --- |
| `tmux` | present at `/home/linuxbrew/.linuxbrew/bin/tmux` | required in local development once harness lands |
| `vim` | present at `/usr/bin/vim` | required |
| `nvim` | present at `/home/linuxbrew/.linuxbrew/bin/nvim` | optional alternative to `vim` |
| `less` | present at `/usr/bin/less` | required |
| `htop` | not found | optional/skip until CI image installs it |
| `btop` | not found | optional/skip until CI image installs it |
| `vttest` | not found | optional/skip until CI image installs it |

Missing binaries must produce explicit `skipped` results, not false successes.
When a binary is promoted to CI-required, the CI image should install it rather
than letting the harness silently skip it.

## Smoke Layers

Use four layers. Each layer should reuse the same case definitions where
possible.

| Layer | Purpose | Runner |
| --- | --- | --- |
| L0 recorded transcript | stable terminal-core regression without external binaries | Rust unit tests feeding `.ansi` fixtures |
| L1 headless PTY app | real local program behavior without GPU or browser | `witty-app` smoke mode backed by `LocalPtyTransport` |
| L2 browser PTY product | browser/gateway parity for selected cases | Playwright through `witty-gateway` or `witty --web` |
| L3 visual capture | nonblank/framing regression for native/window rendering | existing screenshot harness, only for a small subset |

L1 should land before L2. L2 is more expensive and should run fewer cases:
`less` restore, `vim` edit/exit after query replies exist, and `tmux` basic pane
plus OSC 52.

## Harness Contract

Add a reusable real-TUI runner rather than one-off scripts.

Recommended first implementation:

```text
witty --real-tui-smoke <case-id>
```

The runner should:

- spawn a PTY with a deterministic size, initially `24x80`.
- set a controlled environment:
  - `TERM=xterm-256color`
  - `COLORTERM=truecolor`
  - `LC_ALL=C.UTF-8`
  - temp `HOME` or explicit app config flags where possible.
- run with a bounded deadline, initially 10 seconds per case.
- drive named input steps with delays or wait predicates.
- feed all PTY output through `BasicTerminal`.
- drain host actions after each output feed so OSC 52 behavior matches native
  and browser hosts.
- record a JSON report with case id, binary path, version if cheap to collect,
  skip reason, exit status, output byte count, elapsed time, final screen text
  sample, and assertion results.
- never store clipboard payload text or unbounded raw output in smoke
  diagnostics.

Keep raw PTY byte logs opt-in through an env var such as
`WITTY_TUI_SMOKE_CAPTURE_RAW=1`; raw logs can contain user paths, shell
state, or clipboard payloads.

## Case Definitions

### `less-basic-restore`

Purpose:

- alternate-screen enter/leave.
- search and navigation.
- resize-safe pager rendering.

Setup:

- create a temp file with at least 120 deterministic lines.
- run `less -R -M <file>` with `LESS` cleared or controlled.

Input script:

1. wait for line `001` or file name/status marker.
2. send `/Line 050\r`.
3. send `n`.
4. send `q`.

Assertions:

- during the run, visible screen contains a searched line near `Line 050`.
- after `q`, process exits cleanly.
- final main-screen snapshot does not retain full-screen pager content unless
  the shell or runner intentionally printed a post-exit marker.

### `vim-basic-edit`

Purpose:

- full-screen editor startup.
- cursor movement, insert mode, escape, command-line mode, write and quit.
- future DA/DSR/CPR reply coverage.

Setup:

- prefer `vim -Nu NONE -n -i NONE <temp-file>`.
- use `nvim --clean -n <temp-file>` as an optional second case.

Input script:

1. wait for the file text or status line.
2. send `gg0iTF_\x1b`.
3. send `:wq\r`.

Assertions:

- the process exits cleanly.
- the temp file starts with `WITTY_`.
- while active, the snapshot contains either file text or a recognizable status
  line.

Known dependency:

- Some editor builds issue terminal identity and cursor-position queries. If
  they stall or mis-detect capabilities, do not special-case the app first;
  implement the terminal reply path in a separate milestone.

### `tmux-basic-pane`

Purpose:

- nested terminal identity.
- alternate screen inside a multiplexer.
- resize propagation.
- mouse/focus readiness.

Setup:

- use an isolated socket and no user config:
  `tmux -L witty-smoke-<pid> -f /dev/null -u`.
- set `default-terminal` to `tmux-256color` when available, otherwise
  `screen-256color`.

Input script:

1. start or attach a session running a shell that prints `TMUX READY`.
2. send a prefix command to create a second window or split pane.
3. run a small command in the active pane that prints `TMUX PANE OK`.
4. send the tmux detach or exit sequence.

Assertions:

- status line appears while attached.
- expected pane output appears.
- process exits or detaches cleanly.
- no OSC 52 payload appears in terminal cells when tmux clipboard forwarding is
  enabled in a later subcase.

Follow-up subcase:

- `tmux-osc52-copy`: configure tmux clipboard forwarding, trigger an OSC 52
  write from inside the pane, and assert host policy results rather than screen
  text. This should reuse the m139/m140 OSC 52 policy machinery.

### `htop-or-btop-redraw`

Purpose:

- high-frequency full-screen redraw.
- color/style stability.
- function-key exit path.

Setup:

- prefer `htop` if installed; otherwise `btop`; otherwise skip.
- run with a clean config/home.
- isolate `HOME`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, and `XDG_STATE_HOME`.

Input script:

1. wait for CPU/process table text.
2. observe at least two PTY output bursts.
3. send `q`.

Assertions:

- at least two output bursts are observed.
- screen contains a stable process-table marker.
- process exits within the deadline.

The implemented m148 runner registers `htop-or-btop-redraw`, writes an explicit
skip report when both tools are absent, and promotes the same command into a
real redraw smoke once either tool is installed. See
`htop-btop-redraw-smoke.md`.

### `vttest-subset`

Purpose:

- protocol-level coverage that normal apps miss.

Setup:

- only run when `vttest` is installed.
- start with a documented subset, not the full interactive suite.
- replay recorded commands with `vttest -c <commands> 24x80.80` once a verified
  command file exists.
- until that file exists, run only a startup/menu probe and write `skipped` when
  the binary is missing.

Initial subset candidates:

- startup/main menu.
- screen alignment and cursor movement pages.
- character attributes/color pages.
- alternate-screen and scroll-region pages if scriptable.
- device status / cursor-position report pages using the m142 reply path.

Assertions:

- each selected page reaches a known visible marker.
- no panic or timeout.
- failures should identify the page id and last visible marker.

The implemented m147 runner registers `vttest-subset`, provides the missing
binary skip path, supports `WITTY_VTTEST_COMMANDS`, and documents the page
selection in `vttest-subset-smoke.md`. The exact recorded command file remains
deferred until `vttest` is installed in a development or CI image.

## Required Terminal Reply Work

Real editors and `vttest` are likely to expose missing terminal replies. Plan a
small milestone before broad real-TUI automation:

- parse DA requests and reply as a conservative xterm-compatible terminal.
- parse DSR `CSI 5 n` and reply `CSI 0 n`.
- parse CPR `CSI 6 n` and reply with current cursor position.
- route replies through the transport input boundary, not terminal cells.
- keep replies unavailable to plugins by default.
- add native/browser tests proving replies are sent to the child/gateway.

Without this, `vim`, `nvim`, and `vttest` failures may be ambiguous: they could
be app automation problems or missing terminal query behavior.

## Browser Product Selection

Browser real-TUI smokes should be fewer than L1:

| Case | Gateway mode | Reason |
| --- | --- | --- |
| `less-basic-restore` | `witty-gateway` and `witty --web` | validates real PTY output, alternate-screen restore, browser rendering |
| `vim-basic-edit` | `witty-gateway` | validates browser gateway input, editor command-line mode, saved-file behavior |
| `tmux-basic-pane` | `witty --web` | validates product launcher with nested terminal and split-pane input |

Do not run `htop`/`btop` in the default Playwright path until the smoke is fast
and deterministic under CPU load.

## Follow-Up Task Queue

1. `m142-terminal-query-reply-path`
   - done. Implemented DA, DSR, and CPR reply bytes through native and browser
     transport boundaries. See `terminal-query-reply-path.md`.
2. `m143-real-tui-smoke-harness`
   - done. Added the reusable headless PTY smoke runner, case registry, skip
     reporting, JSON artifacts, raw-capture opt-in, and a `less-basic-restore`
     case. See `real-tui-smoke-harness.md`.
3. `m144-vim-less-real-tui-smokes`
   - done. Added `vim-basic-edit` and optional `nvim-basic-edit` to the
     reusable headless PTY runner, kept `less-basic-restore` as the pager
     baseline, and verified all three local cases. See
     `real-tui-smoke-harness.md`.
4. `m145-tmux-real-tui-smoke`
   - done. Added `tmux-basic-pane` with an isolated socket/config, split-pane
     verification, OSC 52 forwarding as a host action, and payload non-rendering
     assertions. See `real-tui-smoke-harness.md`.
5. `m146-browser-real-tui-product-smoke`
   - done. Added a focused Playwright browser/product smoke for
     `less-basic-restore` through both `witty-gateway` and `witty --web`,
     with nonblank canvas screenshots. See `browser-real-tui-product-smoke.md`.
6. `m147-vttest-subset-plan-and-runner`
   - done. Added optional `vttest-subset` to the headless PTY runner, with
     explicit missing-binary skip reporting, `WITTY_VTTEST_COMMANDS`
     command replay support, a default menu-probe path, and
     `vttest-subset-smoke.md`.
7. `m148-htop-btop-real-tui-smoke`
   - done. Added optional `htop-or-btop-redraw` to the headless PTY runner with
     htop-first/btop-fallback selection, isolated config directories, broad
     process-table markers, output-burst assertion, `q` exit path, missing-tool
     skip reporting, and `htop-btop-redraw-smoke.md`.
8. `m149-browser-real-tui-vim-tmux-smokes`
   - done. Added a cached non-reentrant browser screen-read helper, generalized
     `run-witty-web-real-tui-smoke.mjs` to `less-basic-restore`,
     `vim-basic-edit`, and `tmux-basic-pane`, verified vim through Rust
     `witty-gateway`, tmux through product `witty --web`, and preserved the
     original less/browser smoke path. See `browser-real-tui-product-smoke.md`.

## Non-Goals

- Full manual `vttest` certification.
- Installing missing system packages from the smoke runner.
- Recording large raw terminal logs by default.
- Treating skipped optional binaries as success for release gates.
- Broad product UX work such as SSH inventory, SFTP, or AI command blocks.
