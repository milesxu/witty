# Terminal UTF-8 Encoded C1 Aliases

Updated: 2026-06-01

`m438-terminal-utf8-encoded-c1-aliases` extends the C1 normalizer so supported
C1 controls encoded as UTF-8 control-code scalars dispatch as terminal protocol
controls.

Supported examples:

- `C2 85`: NEL
- `C2 8D`: RI
- `C2 9B`: CSI
- `C2 9C`: ST
- `C2 9D`: OSC

The same raw C1 control aliases remain supported. This slice only covers the
C1 controls already implemented by `witty-core`; unsupported C1 bytes are not
expanded into new protocol behavior.

## UTF-8 Boundary

The normalizer withholds a pending `C2` lead byte until the continuation byte is
known. If the continuation byte is a supported C1 control, it emits the
equivalent raw C1 or 7-bit ESC sequence. If the continuation byte is printable
Latin-1, such as `A3` for `£`, the original UTF-8 bytes are passed through.

Other multi-byte UTF-8 text remains unchanged, including characters whose later
continuation bytes happen to equal C1 byte values.

## Verification

- `cargo test -p witty-core utf8_encoded_c1 --quiet`
- `cargo test -p witty-core c1_ --quiet`
- `cargo test -p witty-core --quiet`
