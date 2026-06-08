# Terminal XTGETTCAP Capabilities

`m456-terminal-xtgettcap-static-capabilities` adds a minimal xterm-compatible
`witty-core` reply path for XTGETTCAP:

```text
DCS + q Pt ST
```

`Pt` is parsed as semicolon-separated hexadecimal capability names. Replies are
queued as `TerminalHostAction::TerminalReply`, not rendered as terminal cells:

```text
DCS 1 + r name-hex = value-hex ST
DCS 1 + r name-hex ST
DCS 0 + r name-hex ST
```

String and numeric capabilities include `= value-hex`. Boolean capabilities use
the success reply without a value.

## Static Capability Set

Witty currently answers a conservative static subset aligned with the
`xterm-256color` environment used by the launcher and smoke tests:

| Name | Reply |
| --- | --- |
| `TN`, `name` | `xterm-256color` |
| `Co`, `colors` | `256` |
| `RGB` | `8/8/8` |
| `Tc` | boolean true |
| `Cs`, `Cr` | OSC 12 cursor-color set/reset strings |
| `Ms` | OSC 52 clipboard write terminfo string |
| `Ss`, `Se` | DECSCUSR cursor-style set/reset strings |
| `sitm`, `ritm` | italic enter/exit strings |
| `Smulx` | underline-style SGR string |
| `Setulc` | underline-color SGR string |
| `Sync` | synchronized-output mode string |
| `BE`, `BD` | bracketed-paste enable/disable strings |
| `fe`, `fd` | focus-event enable/disable strings |
| `kxIN`, `kxOUT` | focus-in/focus-out input sequences |

Unknown valid names return `DCS 0 + r name-hex ST`. Malformed hex names,
decoded control characters, empty names, and payloads over 512 bytes are ignored
instead of echoed through the host boundary. The larger XTGETTCAP limit keeps
multi-capability probes working while DECRQSS keeps its stricter 64-byte cap.

## Boundary

This is a static terminal-emulator capability contract. It does not read the
host terminfo database, expose host OS details, answer arbitrary terminfo keys,
or implement XTSETTCAP mutation. The goal is compatibility with common tmux,
Neovim, and shell probes while keeping replies deterministic.

## Verification

- `cargo test -p witty-core xtgettcap --quiet`
- `cargo test -p witty-core decrqss --quiet`
- `cargo test -p witty-core c1_dcs --quiet`
