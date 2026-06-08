# Safe Host Sync Plan

Updated: 2026-06-01

## Current State

Local Witty worktree:

| Field | Value |
| --- | --- |
| Path | `/home/mingxu/src/witty` |
| Branch | `master` |
| Remote | `git@github.com:milesxu/witty.git` |
| Dirty entries | migration worktree; inspect with `git status --short` before sync |

aibookmx preflight:

| Field | Value |
| --- | --- |
| Git | `/usr/bin/git` |
| rsync | `/usr/bin/rsync` |
| gh | `/usr/bin/gh` |
| Rust | installed under `/home/xuming/.cargo/bin` |
| Worktree | `~/src/witty` missing |
| SSH display | no `DISPLAY` or `WAYLAND_DISPLAY` |

## Recommended Sync Route

Use Git, not rsync, for the safe-host validation baseline:

1. Classify the current dirty worktree into keep/exclude/delete groups.
2. Commit the meaningful Witty changes locally after user confirmation.
3. Push `main` to the private GitHub remote.
4. Clone or fetch on aibookmx:

```text
git clone git@github.com:milesxu/witty.git ~/src/witty
```

5. Run non-graphical validation over SSH:

```text
cargo test -p witty-render-wgpu
cargo test -p witty-app
cargo run -p witty-app -- --renderer-backend-info
cargo run -p witty-app -- --renderer-no-surface-diagnostics
```

6. Run real native OpenGL window validation only from a graphical aibookmx
session, using:

```text
env WGPU_BACKEND=gl cargo run -p witty-app -- --window --window-startup-report --window-exit-after-ms 1200
```

## Why Not rsync First

Rsync can copy the dirty tree, but it creates a second unreviewed mutable copy
of dozens of dirty entries. That makes validation results harder to tie back to a
commit and increases the risk of testing code that is not recoverable after
another reboot.

Use rsync only for throwaway experiments after deciding which uncommitted files
belong in the validation snapshot.

## Approval Boundary

No sync has been performed. The next write step should be either:

- user-approved local commit and push, then clone/fetch on aibookmx; or
- user-approved rsync to a named throwaway directory on aibookmx.
