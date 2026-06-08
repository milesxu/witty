# Htop/Btop Redraw Smoke

Updated: 2026-05-30

`m148-htop-btop-real-tui-smoke` adds an optional real-TUI redraw case:

```text
witty --real-tui-smoke htop-or-btop-redraw
```

The case prefers `htop` when available and falls back to `btop`. If neither
binary is installed, it writes an explicit skipped report instead of treating
the case as passed by omission.

## Runner Behavior

The case:

- searches `PATH` for `htop`, then `btop`.
- writes `status=skipped` with `skip_reason="neither htop nor btop found"` when
  neither tool is available.
- spawns the selected tool in a `24x80` PTY.
- uses isolated `HOME`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, and
  `XDG_STATE_HOME` directories so user configuration is not read or mutated.
- sets `TERM=xterm-256color`, `COLORTERM=truecolor`, and `LC_ALL=C.UTF-8`.
- waits for process-table or CPU/memory markers.
- requires at least two PTY output bursts to prove the app is actively drawing.
- sends `q` and requires the process to exit with status `0`.

## Markers

The smoke intentionally checks broad text markers rather than tool-specific
layout coordinates:

| Tool | Accepted marker style |
| --- | --- |
| `htop` | `PID` plus `Command`, `USER`, `Load average`, or `Tasks` |
| `btop` | `btop`, or `cpu` plus `mem`, or `proc` plus `pid` |

This avoids overfitting the smoke to one release theme while still catching
blank-screen, parser, and exit-key regressions.

## Local Result

On this machine both tools are currently missing, so the expected result is:

```text
Real TUI smoke htop-or-btop-redraw status=skipped artifact=target/real-tui-smoke/htop-or-btop-redraw.json
```

Once CI installs either tool, this same case becomes an actual redraw smoke
without changing the command line.
