# Update And Restart State

Updated: 2026-06-08

Witty's local development installer writes an installed-build marker after a
successful user-local install. Native installed windows poll the marker and
show a `Restart to update` action when the installed marker build id differs
from the build id captured at window startup.

The marker path is deterministic for tests and fake homes:

```text
$XDG_STATE_HOME/witty/install-state.v1.json
~/.local/state/witty/install-state.v1.json
```

The restart action writes a v1 snapshot next to the marker and starts the
installed binary with:

```text
witty --window --restore-state <snapshot>
```

The snapshot stores only launch and interface metadata: grid dimensions,
optional window pixel size, active tab index, tab source/profile ids, local
program/args/cwd, safe environment metadata, and SSH/profile launch metadata
where Witty has it. Safe environment values are limited to `TERM`,
`COLORTERM`, `LANG`, `LC_*`, and `WITTY_*`; other keys may be recorded as
redacted metadata without values.

The snapshot does not store terminal text, scrollback, clipboard payloads,
secrets, or raw PTY output. Ordinary local shells and programs are relaunched
after restart. Lossless PTY child process continuity requires an external
multiplexer such as tmux, or a future persistent Witty daemon design.
