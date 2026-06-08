# Terminal OSC 52 Native Clipboard

Updated: 2026-05-30

m139 connects terminal-core OSC 52 host actions to the native `winit` window.
The system clipboard remains protected by an explicit native-only policy.

## Implemented Surface

Native CLI flag:

```text
--osc52-clipboard disabled|confirm|allow
```

Behavior:

- default is `disabled`.
- the flag is accepted only with `--window`.
- `disabled` drains and drops OSC 52 clipboard actions without writing the
  system clipboard.
- `confirm` drains actions and rejects writes until a real confirmation UI
  exists.
- `allow` is the only policy that calls `ClipboardSink::set_text()`.

The native output polling loop now drains `BasicTerminal::drain_host_actions()`
immediately after feeding PTY output. Clipboard payloads still do not enter
terminal cells, scrollback, plugin events, command arguments, diagnostics, or
render plans.

## Clipboard Targets

OSC 52 clipboard target `c` maps to the normal system clipboard.

OSC 52 target `p` maps to the Linux primary selection on Linux-like Unix
targets. Other targets reject primary selection before calling the clipboard
sink.

## Verification

Passed:

- `cargo fmt --all`
- `cargo test -p witty-app osc52 --quiet`
- `cargo test -p witty-app app_options --quiet`
- `cargo test -p witty-app --quiet`
- `cargo test -p witty-core osc52 --quiet`

## Next Step

`m140-browser-osc52-clipboard-smoke` should expose host action draining through
the wasm/browser path and apply the browser Clipboard API policy boundary with
Playwright coverage.
