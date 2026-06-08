# Plugin Runtime Error Isolation

This slice separates plugin-owned event handler failures from host policy
violations.

## Contract

`PluginHost` records event handler failures as `PluginRuntimeFailure`:

- `plugin_id`
- `kind`
- `message`

The first implemented kind is `EventHandlerError`, covering built-in plugin
handler errors and Wasm `handle-event` traps/errors once they reach the shared
host boundary.

When a plugin fails while handling an event, the host:

- records the runtime failure;
- disables that plugin for later dispatch;
- continues dispatching the same event to other eligible plugins;
- returns actions from healthy plugins.

If the disabled plugin owns a command id, later `CommandInvoked` events for
that command id return no actions. They are not broadcast to other plugins and
the disabled plugin is not retried.

## Still Fail Loud

Host policy violations remain hard errors because they indicate a bug or unsafe
contract breach in the host/plugin boundary:

- registering a command for a different `source_plugin`;
- duplicate install-time or runtime command ids;
- runtime command ids colliding with the app-visible command registry;
- terminal writes without `TerminalWritePermission::AllowSession`.

Those errors still return `Err` from dispatch and do not silently downgrade into
runtime failures.

## Product Surface

`TerminalApp` exposes:

- `disabled_plugin_ids()`
- `plugin_runtime_failures()`

This is intentionally just state exposure for now. UI surfacing, retry/reload,
and plugin crash-count backoff remain later product work.

## Verification

- `cargo test -p witty-ui plugin_event_handler_errors_disable_plugin_and_continue_dispatch --quiet`
- `cargo test -p witty-ui disabled_command_owner_is_not_broadcast_or_retried --quiet`
- `cargo test -p witty-ui plugin_host --quiet`
