# Terminal Alternate Screen Core

Updated: 2026-05-30

## Scope

`BasicTerminal` supports DEC private mode `1049` for alternate screen
switching:

- `CSI ? 1049 h`: enter alternate screen, save main cursor, clear alternate
  cells, and reset alternate cursor position to `(0, 0)`.
- `CSI ? 1049 l`: leave alternate screen, restore saved main cursor position,
  and return to the main screen buffer.

The renderer and app-facing `RenderSnapshot` shape remain unchanged. The active
buffer is selected inside `BasicTerminalState` before producing a snapshot.

## Buffer Model

The terminal core now owns two `ScreenBuffer` values:

- `main`: normal shell screen plus main scrollback.
- `alternate`: isolated full-screen app buffer with no scrollback.

`ActiveScreen` selects which buffer receives output, cursor movement, erase, and
resize operations.

Main scrollback and viewport offset remain main-screen-only. While alternate
screen is active, viewport scrolling is ignored and alternate scrollback does
not append to main history.

## Global State

These remain terminal-global across screen switches:

- current SGR style
- bracketed paste mode
- OSC title
- cursor visibility
- cursor shape

Full terminal reset exits alternate screen and resets both buffers.

## Follow-Ups

- Decide whether cursor save/restore should later include more DEC cursor state
  than position.
- Keep browser gateway smoke coverage in sync with
  `docs/browser-alt-screen-runtime-smoke.md` as new alternate-screen modes are
  added.
