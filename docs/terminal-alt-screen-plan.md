# Terminal Alternate Screen Plan

Updated: 2026-05-30

## Goal

Add alternate screen buffer support to `BasicTerminal` without changing renderer
or app-facing frame APIs. Full-screen terminal applications such as editors,
pagers, TUIs, and shells using `less`, `vim`, `top`, or `fzf` should be able to
enter an isolated screen, draw there, and return to the original main screen.

## Supported Sequences

Initial implementation should support the sequences most commonly emitted by
modern terminal applications:

- `CSI ? 1049 h`: enter alternate screen, save cursor, clear alternate screen.
- `CSI ? 1049 l`: leave alternate screen, restore cursor, return to main screen.

Compatibility follow-up:

- `CSI ? 1047 h/l`: enter/leave alternate screen without cursor save semantics.
- `CSI ? 1048 h/l`: save/restore cursor only.
- `CSI ? 47 h/l`: older alternate-screen alias, probably map to the `1047`
  behavior once tests cover it.

Start with `1049` in m78 because it covers the dominant behavior from full-screen
apps and minimizes ambiguous legacy behavior.

## Data Model

Current state is stored directly on `BasicTerminalState`:

- `cells`
- `scrollback`
- `viewport_offset`
- `selection`
- `cursor`

Alternate screen support should split visible buffer state from terminal-global
state. Proposed shape:

```rust
#[derive(Clone, Debug)]
struct ScreenBuffer {
    cells: Vec<Vec<BasicCell>>,
    cursor: CursorState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActiveScreen {
    Main,
    Alternate,
}
```

`BasicTerminalState` should then own:

- `main: ScreenBuffer`
- `alternate: ScreenBuffer`
- `active_screen: ActiveScreen`
- `saved_main_cursor: Option<CursorState>`
- `scrollback`: main-screen-only history
- `viewport_offset`: main-screen-only viewport scrollback offset
- `selection`: active-view selection, cleared on buffer switch
- terminal-global state such as `current_style`, `title`, bracketed paste, damage

Helper accessors should hide the active-buffer indirection:

- `active_buffer(&self) -> &ScreenBuffer`
- `active_buffer_mut(&mut self) -> &mut ScreenBuffer`
- `cells(&self)`, `cells_mut(&mut self)` or direct helpers for row/cell access
- `cursor(&self)`, `cursor_mut(&mut self)` if borrowing stays manageable

This keeps rendering unchanged because `snapshot()` still emits one visible
`RenderSnapshot`.

## Behavior Rules

Entering alternate screen with `1049h`:

- call `follow_tail()` first so main viewport is not left scrolled back
- save the current main cursor
- set `active_screen = Alternate`
- reset alternate cells to blank rows sized to the current grid
- reset alternate cursor to default at `(0, 0)`
- clear selection
- mark full damage

Leaving alternate screen with `1049l`:

- set `active_screen = Main`
- restore saved main cursor if present
- clear selection
- force `viewport_offset = 0`
- mark full damage

While alternate screen is active:

- output, cursor movement, erase operations, and resize affect only alternate
  cells/cursor
- `scroll_up()` rotates alternate rows and does not append to main scrollback
- mouse-wheel viewport scroll should remain at `0` because alternate screen has
  no scrollback in the first implementation
- `CSI 3 J` should clear main scrollback only when on the main screen; on the
  alternate screen it should leave main scrollback intact unless compatibility
  testing proves otherwise

Terminal-global state should persist across buffer switches:

- SGR current style
- bracketed paste mode
- OSC title
- cursor visibility and shape, unless a later DEC save/restore model requires
  splitting this more precisely

## Resize Strategy

Resize both buffers to the new grid size. Preserve overlapping cell content in
each buffer independently, clamp both cursors, and mark full damage.

Main scrollback remains main-only. Alternate screen resize should not create
scrollback entries.

## Test Plan For m78

Core tests:

- entering `1049h` hides existing main cells and shows a blank alternate buffer
- writing in alternate buffer does not mutate main buffer
- leaving `1049l` restores main cells and saved cursor
- alternate scroll at bottom rotates alternate rows without adding main
  scrollback
- viewport scrolling while alternate is active keeps `viewport_offset() == 0`
- resize preserves/clamps both main and alternate buffers independently
- OSC title and SGR current style survive entering/leaving alternate screen
- full reset exits alternate screen and clears both buffers

Regression checks:

- `cargo test -p witty-core -- --nocapture`
- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo check -p witty-web --target wasm32-unknown-unknown`

## Implementation Notes

This refactor touches many existing methods in `basic_terminal.rs` because most
helpers currently read/write `self.cells` and `self.cursor` directly. Keep m78
limited to `witty-core` plus documentation unless app or renderer compile breaks
require API changes.

Avoid implementing a full DEC private mode matrix in m78. Add named constants
for `1049`, route through `set_private_mode`, and leave `1047`/`1048`/`47` for a
small follow-up once the main buffer abstraction is stable.
