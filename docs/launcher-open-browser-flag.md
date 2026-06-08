# Launcher Open Browser Flag

Updated: 2026-05-30

## Purpose

`witty --web` prints the local browser URL by default. Product usage should
also support opening the system browser explicitly, while tests and headless
runs remain deterministic.

## CLI

```text
witty --web --open-browser
```

The flag is opt-in. Without it, the launcher only prints:

```text
Witty launcher listening on http://127.0.0.1:<port>/index.html#session=<id>
```

## Platform Commands

The opener is intentionally dependency-free:

- Linux/Unix: `xdg-open <url>`
- macOS: `open <url>`
- Windows: `cmd /C start "" <url>`

The launcher spawns the opener and continues serving the UI/gateway. If the
opener command cannot be started, `--open-browser` fails loudly.

## Test Boundary

Unit tests verify CLI parsing and opener command selection without launching a
GUI process. Browser smoke does not pass `--open-browser`; it still controls
Chromium through Playwright.
