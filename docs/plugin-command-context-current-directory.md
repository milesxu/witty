# Plugin Command Context

This slice connects shell metadata to plugin command invocations without making
terminal contents broadly plugin-visible.

## Contract

`witty-plugin-api` now gives `CommandInvocation` a `context` field:

- `CommandInvocationContext::current_directory`
- `CommandInvocationContext::selected_command_block`
- `PluginCurrentDirectory { uri, host, path }`
- `PluginCommandBlock { id, command_range, output_range, exit_code,
  started_at_ms, finished_at_ms, duration_ms, current_directory }`
- `PluginCommandBlockTextRange { start, end_exclusive }`

The Wasm Component Model WIT mirrors this with `command-invocation-context` and
`current-directory` plus `command-block` records. `args-json` remains unchanged,
so command schemas can still evolve independently from the ABI.

## Source Of Truth

`OSC 7 ; file://host/path ST/BEL` is parsed in `witty-core` and stored in
`ShellIntegrationState`. Native and browser command invocation paths read the
latest current directory for the active terminal screen and pass it through
`TerminalApp::invoke_command_with_context()`.

When a completed OSC 133 command block is selected on the active screen, the
same context includes execution and range metadata for that selected block.
Ranges use active-screen terminal coordinates and end-exclusive bounds. The
block metadata intentionally excludes command text and output text;
block-scoped text remains behind explicit product commands such as
copy-command/copy-output.

## Permission Boundary

`PluginHost` filters context per plugin before dispatch:

- plugins with `TerminalReadPermission::None` receive an empty command context;
- plugins with `TerminalReadPermission::SelectionOnly` receive selected
  command-block metadata only;
- plugins with `TerminalReadPermission::CurrentScreen` or `FullScrollback`
  receive current-directory and selected-command-block metadata.

The same permission gate applies to read events:

- `TerminalOutput` is delivered only to `CurrentScreen` and `FullScrollback`
  plugins;
- `SelectionChanged` is delivered to `SelectionOnly`, `CurrentScreen`, and
  `FullScrollback` plugins.

Command invocations are not broadcast. `PluginHost` resolves
`CommandRegistration::source_plugin` for the invoked command id and sends the
event only to that plugin. Unknown command ids are dropped at the plugin-host
boundary; `TerminalApp` still validates command ids before invoking the host in
normal UI paths.

Runtime `PluginAction::RegisterCommand` actions update the same plugin-host
ownership table after host-side validation. Duplicate dynamic command ids are
rejected, so newly registered commands can be routed without falling back to
broadcast. When dispatch runs through `TerminalApp`, the app command registry is
also passed as a reserved set, so runtime registrations cannot collide with
commands already visible to product UI before host and app state are mutated.

This keeps cwd metadata out of default plugin events while still enabling
explicitly trusted workflow plugins to implement commands such as rerun,
open-in-editor, or project-aware actions.

## Verification

- `cargo test -p witty-plugin-api --quiet`
- `cargo test -p witty-ui context --quiet`
- `cargo test -p witty-ui invoke_command_with_context --quiet`
- `cargo test -p witty-ui runtime_registered_command_joins_app_and_host_routing --quiet`
- `cargo test -p witty-ui runtime_registered_command_cannot_collide_with_app_registry --quiet`
- `cargo test -p witty-plugin-wasm api_command_event --quiet`
- `cargo test -p witty-plugin-wasm fixture_component_exports_are_callable --quiet`
