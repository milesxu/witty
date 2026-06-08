# Terminal Current Directory OSC 7

This slice adds the shell current-directory signal used by modern terminals and
shell integrations:

```text
OSC 7 ; file://host/path ST
OSC 7 ; file://host/path BEL
```

## Behavior

- `witty-core` consumes OSC 7 without writing its payload into terminal cells.
- Only `file://` URIs are accepted. Other URI schemes are ignored.
- The URI is preserved verbatim in `TerminalCurrentDirectory::uri`.
- The host field is optional and decoded from the URI authority when present.
- The path is percent-decoded as UTF-8 and rejected if it contains control
  characters.
- The host action carries the current screen, cursor point, and row anchor so UI
  layers can associate the directory with the current shell prompt or command
  context later.

## Host Boundary

`TerminalHostAction::CurrentDirectory` is a host-owned signal. Native and
browser host-action drains store the latest directory in `ShellIntegrationState`;
they do not write to the PTY, touch the clipboard, or mutate terminal screen
contents.

When OSC 133 command-block markers are present, `ShellIntegrationState` also
copies the current directory onto the pending block and then onto the completed
block. A directory received after prompt start still updates the pending block
for the same terminal screen.

Plugin command invocations can receive this metadata through permission-gated
command context. See `plugin-command-context-current-directory.md`.

## Verification

- `cargo test -p witty-core osc7 --quiet`
- `cargo test -p witty-ui current_directory --quiet`
- `cargo test -p witty-app current_directory_host_actions --quiet`
- `cargo test -p witty-web current_directory --quiet`
