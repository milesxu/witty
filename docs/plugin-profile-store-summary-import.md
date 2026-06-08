# Plugin Profile Store Summary Import

This slice adds the first permission-gated Wasm host import that exposes
product data. The import is intentionally read-only and summary-only.

## Contract

The WIT host interface now includes:

```wit
record profile-store-summary {
  profile-count: u32,
  default-profile-configured: bool,
  launchable-profiles: u32,
  credential-resolver-required-profiles: u32,
}

interface host {
  get-profile-store-summary: func() -> option<profile-store-summary>;
}
```

The host returns `none` unless both conditions are true:

- the plugin manifest requests `profile-read`;
- the native host state has a profile summary configured for the plugin
  instance.

## Privacy Boundary

The import exposes only counts and whether a default profile is configured.

It deliberately does not expose:

- profile ids, names, descriptions, tags, or grouping;
- target host, user, port, proxy jump, or connection strings;
- SSH config paths, identity paths, extra SSH arguments, or environment values;
- credential resolver ids or vault references;
- profile store path, default profile id, or raw `ProfileStoreV1` content.

The summary is suitable for capability-aware UI decisions, such as showing a
profile command only when launchable profiles exist. Opening a specific profile
or reading detailed profile metadata should use a separate host-owned flow with
its own permission and confirmation policy.

## Runtime Integration

`WasmPluginState` stores manifest permissions after `manifest()` is loaded.
`get-profile-store-summary()` checks that stored permission state before
returning the optional host-provided summary.

`witty-ui` owns the bridge from the richer transport-layer
`ProfileStoreSummary` to the plugin ABI summary:

- `plugin_profile_store_summary_from_redacted()` maps only counts and default
  presence.
- `PluginHost::prepare_wasm_component_from_file_with_profile_store_summary()`
  injects the mapped summary into `WasmPluginState`.
- `TerminalApp::install_wasm_plugin_from_file_with_profile_store_summary()`
  exposes the same injection path at the app boundary.

This keeps `witty-plugin-wasm` independent from profile-store storage types and
keeps the privacy filtering at the host/app layer.

The fixture plugin includes `fixture.profile-summary`, which calls the import
and writes either `profiles none` or the summary counts through the existing
`WriteTerminal` action path. The host still enforces terminal-write permission
before bytes reach the transport.

## Verification

- `cargo test -p witty-plugin-api --quiet`
- `cargo test -p witty-plugin-wasm --quiet`
- `cargo test -p witty-ui --quiet`
