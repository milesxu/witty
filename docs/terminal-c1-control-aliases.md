# Terminal C1 Control Aliases

`m354-terminal-c1-control-aliases` adds pure `witty-core` handling for common
8-bit C1 control bytes that alias already-supported ESC controls.

## Supported Aliases

`BasicTerminal` now maps:

| C1 byte | Name | Equivalent existing control |
| --- | --- | --- |
| `0x84` | IND | `ESC D` |
| `0x85` | NEL | `ESC E` |
| `0x88` | HTS | `ESC H` |
| `0x8d` | RI | `ESC M` |
| `0x96` | SPA | `ESC V` |
| `0x97` | EPA | `ESC W` |
| `0x9a` | DECID | `ESC Z` |

Existing `0x8e` SS2 and `0x8f` SS3 handling remains unchanged.

The `m438-terminal-utf8-encoded-c1-aliases` follow-up also recognizes these
execute aliases when encoded as UTF-8 C1 control-code scalars. For example,
`C2 85` dispatches as NEL and `C2 8D` dispatches as RI. Printable Latin-1 text
with the same `C2` lead byte remains printable text.

## Behavior Boundary

These C1 controls reuse the existing terminal-core implementation paths:

- IND uses the same index/linefeed behavior as `ESC D`.
- NEL performs carriage return plus linefeed, matching `ESC E`.
- HTS sets a horizontal tab stop at the current column, matching `ESC H`.
- RI performs reverse index and respects scroll regions, matching `ESC M`.
- SPA/EPA toggle the current protected-cell attribute for future printed cells,
  matching the existing selective-erase protection path.
- DECID sends the same primary device-attributes reply as `ESC Z`.

No renderer, PTY transport, browser, terminal reply, plugin, or host action
surface changes are required.

## Verification

- `cargo test -p witty-core c1_ --quiet`
- `cargo test -p witty-core utf8_encoded_c1 --quiet`
- `cargo test -p witty-core esc_index --quiet`
- `cargo test -p witty-core reverse_index --quiet`
