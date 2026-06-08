# VTTEST Subset Smoke

Updated: 2026-05-30

`m147-vttest-subset-plan-and-runner` adds an optional `vttest-subset` case to
the real-TUI smoke runner:

```text
witty --real-tui-smoke vttest-subset
```

Local `vttest` is not installed, so the default local result is an explicit
`skipped` JSON report. This keeps the case visible in the registry without
making development machines or CI images install the tool before the exact
subset is finalized.

## Runner Behavior

The case:

- searches `PATH` for `vttest`.
- writes `status=skipped` with `skip_reason="vttest not found"` when the binary
  is missing.
- spawns the binary in a `24x80` PTY with `TERM=xterm-256color`,
  `COLORTERM=truecolor`, `LC_ALL=C.UTF-8`, and an isolated `HOME`.
- passes `24x80.80` so the smoke does not depend on an 80/132-column switch.
- waits for a startup/menu marker such as `VTTEST`, `VT100`, `Choose test`, or
  `Test of cursor`.
- sends `0\r` to exit when no command replay file is configured.
- records output counters, final screen sample, exit status, and assertion
  details in `target/real-tui-smoke/vttest-subset.json`.

The Debian vttest manpage documents `-V`, geometry arguments such as
`24x80.80`, and `-c commands` for replaying command files recorded by `-l`:
https://manpages.debian.org/unstable/vttest/vttest.1

## Recorded Subset Path

Set `WITTY_VTTEST_COMMANDS=<path>` to use a recorded vttest command replay
file:

```text
WITTY_VTTEST_COMMANDS=docs/fixtures/vttest-subset.commands \
  witty --real-tui-smoke vttest-subset
```

When this env var is set, the runner invokes:

```text
vttest -c <commands> 24x80.80
```

The command file is intentionally not checked in yet because the local machine
does not have `vttest` installed to record and verify it. Once the binary is
available in a development or CI image, record a narrow subset with `vttest -l`
and promote that file only after the replay passes.

## Initial Page Selection

The first recorded subset should stay small and deterministic:

| Page area | Why include it | Expected assertion style |
| --- | --- | --- |
| Startup/main menu | proves geometry, startup parsing, and menu rendering | visible `VTTEST`/menu marker |
| Cursor movement | catches CUP/HVP, wrap, and row/column addressing regressions | page marker plus no timeout |
| Screen alignment or erase | catches line/cell clearing regressions | page marker plus no panic |
| Character attributes/color | catches SGR/color regressions outside app-specific behavior | page marker plus no payload leak |
| Device status/cursor reports | exercises m142 DA/DSR/CPR reply path | output continues past query page |

Do not run the full interactive suite in the default smoke. Keep broad manual
certification separate from the fast real-TUI regression path.

## Verification

Expected on this machine until `vttest` is installed:

```text
Real TUI smoke vttest-subset status=skipped artifact=target/real-tui-smoke/vttest-subset.json
```
