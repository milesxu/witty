# Plugin Host Info Import

This slice adds the first non-empty Wasm host import surface while keeping the
plugin ABI deliberately narrow.

## Contract

The WIT world imports `witty:plugin/host`:

```wit
interface host {
  get-host-info: func() -> host-info;
}
```

`host-info` contains only:

- `app-name`
- `app-version`
- `plugin-abi-version`

The default native Wasmtime state returns `Witty`, the current crate
version, and `witty_plugin_api::PLUGIN_ABI_VERSION`.

## Privacy Boundary

`get-host-info` intentionally does not expose:

- OS username, hostname, or machine id;
- current directory, profile id, SSH host, user, port, or paths;
- PTY command, environment variables, or process ids;
- renderer backend, GPU adapter, window system, or browser details;
- plugin registry contents or other plugin ids.

Those values may be useful later, but each needs a separate permission and UI
policy decision. This import is only for ABI negotiation and user-facing plugin
diagnostics.

## Runtime Integration

`WasmPluginRuntime::host_linker()` registers the generated host imports by
default. `instantiate_component()` now uses that linker, so ordinary plugin
loading supports host imports without callers wiring a custom linker.

Tests keep `empty_linker()` available for negative/custom-linker cases, but the
product path should use the host linker.

The fixture plugin has a `fixture.host-info` command that calls the import and
writes the values back through the existing permission-gated `WriteTerminal`
action. This verifies the full guest import -> host state -> guest action path.

## Verification

- `cargo test -p witty-plugin-api --quiet`
- `cargo test -p witty-plugin-wasm --quiet`
