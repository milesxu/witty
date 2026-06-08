# Terminal Title OSC

Updated: 2026-05-30

## Scope

`BasicTerminal` now handles common OSC title sequences:

- `OSC 0 ; title BEL` and `OSC 0 ; title ST`
- `OSC 2 ; title BEL` and `OSC 2 ; title ST`

The parsed title is stored in `RenderSnapshot.title` and exposed through
`BasicTerminal::title()` and `TerminalApp::title()`.

## Behavior

OSC `0` and `2` both update the terminal window title. OSC `1` icon-title
updates and unrelated OSC commands are ignored for now.

Titles are decoded with UTF-8 loss replacement and C0/C1 control characters are
filtered. Semicolons after the OSC code are preserved as part of the title.

Title changes do not mark terminal rows dirty because they do not affect the
terminal grid. Native window mode synchronizes `TerminalApp::title()` into the
OS window title, falling back to the default Witty title for `None` or an
empty title.

## Follow-Ups

- Track icon title separately if a platform integration needs OSC `1`.
