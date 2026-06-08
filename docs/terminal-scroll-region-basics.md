# Terminal Scroll Region Basics

Updated: 2026-06-01

## Scope

`BasicTerminal` supports the DECSTBM scroll-margin sequence:

- `CSI top ; bottom r`: set a 1-based inclusive scrolling region.
- `CSI r`: reset the region to the full screen.

Invalid regions where `top >= bottom` are ignored. Setting a valid region moves
the cursor to the home position, matching common terminal behavior.
Missing and explicit `0` margin parameters use their defaults, so `CSI 0;0 r`
resets the effective region to the full screen.

## Behavior

Linefeed at the bottom margin scrolls only the configured region. Rows above
and below the region are preserved, which is required by many full-screen TUI
layouts with fixed headers or footers.

When the effective region is the full screen, scrolling keeps the previous
behavior and appends the outgoing first row to main scrollback. Partial-region
scrolling does not append to scrollback.

Full reset and terminal resize clear the custom region back to the full screen.
