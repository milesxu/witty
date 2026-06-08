# Profile Store CLI Plan

Updated: 2026-06-01

## Purpose

`m231-profile-store-cli-or-import-plan` chose the first product surface for
using the locked `ProfileStoreV1` edit helpers implemented in m230.

Decision: implement a narrow native CLI profile-management surface before
building host-owned UI commands or OpenSSH config import.

The goal is to make profile storage usable and testable without introducing a
browser-side editor, plugin write permission, vault resolver, or config importer
in the same slice.

## Decision

Start with native CLI commands in `witty-app`.

Rationale:

- It directly exercises `edit_profile_store()` and its read-modify-write lock
  contract.
- It is deterministic to test without GUI automation, real SSH hosts, network
  access, or browser clipboard permissions.
- It keeps profile writes in trusted native code. Browser sessions still receive
  only the redacted gateway session config.
- It gives future UI commands and import preview flows a stable host-owned
  persistence boundary to call.

Do not start with OpenSSH config import. Import needs parsing, conflict
resolution, warning presentation, and user confirmation. Those are easier to
design once add/list/remove/default already exist as product primitives.

Do not start with browser or plugin editing. Plugins should not receive raw
profile stores or write access by default, and browser UI should not be the
first owner of local filesystem persistence.

## Initial CLI Surface

Keep the first surface flag-based to match the existing hand-written parser:

```text
witty --profile-store-list [--profile-store <path>]

witty --profile-store-add \
  --ssh-profile-json <path> \
  [--profile-store <path>] \
  [--set-default]

witty --profile-store-update \
  --ssh-profile-json <path> \
  [--profile-store <path>]

witty --profile-store-remove <id> [--profile-store <path>]

witty --profile-store-check-launch <id> [--profile-store <path>]

witty --profile-store-set-default <id> [--profile-store <path>]

witty --profile-store-clear-default [--profile-store <path>]
```

`--profile-store <path>` keeps its existing launcher meaning under `--web`, but
without `--web` it selects the store file for profile-management commands. If
omitted, native profile-management commands use `default_profile_store_path()`.

`--ssh-profile-json <path>` keeps its existing launcher meaning under `--web`,
but in `--profile-store-add` or `--profile-store-update` mode it is the input
profile JSON. Mixing `--web` with profile-management commands should fail.

## Command Semantics

`--profile-store-list`:

- reads the selected store path
- if the default path is used and the file is missing, prints an empty list
- if an explicit path is missing, fails loudly
- prints only redacted inventory columns:
  - default marker
  - id
  - name
  - tags
  - launchability
- does not print host, user, port, jump host, identity-file path, config-file
  path, OpenSSH argv, remote command, credential secret id, or serialized JSON

`--profile-store-add`:

- loads one `SshProfile` from `--ssh-profile-json`
- uses `edit_profile_store(..., CreateIfMissing, ...)`
- applies `ProfileStoreDefaultPolicy::SetIfEmpty` by default
- applies `ProfileStoreDefaultPolicy::SetToAdded` when `--set-default` is used
- fails on duplicate id

`--profile-store-update`:

- loads one `SshProfile` from `--ssh-profile-json`
- uses `edit_profile_store(..., Existing, ...)`
- updates by the profile's own id
- rejects id changes by relying on `ProfileStoreV1::update_profile()`

`--profile-store-remove`:

- uses `edit_profile_store(..., Existing, ...)`
- removes the requested id
- clears `default_profile_id` only when the removed profile was default
- does not silently select a replacement default

`--profile-store-set-default`:

- uses `edit_profile_store(..., Existing, ...)`
- requires the id to already exist

`--profile-store-clear-default`:

- uses `edit_profile_store(..., Existing, ...)`
- clears the default without modifying profiles

## Output Contract

Human output should be stable enough for smoke tests but not treated as a full
machine API yet.

Suggested list format:

```text
default id        name        launchability                  tags
*       prod      Production  launchable                     work,linux
        vaulted   Vaulted     requires-credential-resolver   secure
```

Suggested mutation output:

```text
profile store updated: changed=true profiles=2 default_changed=false bytes=912
```

Mutation reports should use the existing non-sensitive
`ProfileStoreEditReport` fields:

- changed
- profile count
- default changed
- bytes written
- created parent dir

Do not print profile host/user/path/argv/remote-command values after mutation.

## Parser Placement

Add a new `AppMode::ProfileStore(ProfileStoreCommand)` in `witty-app`.

Keep `witty-launcher` focused on `--web` launch. The launcher should continue to
accept only launch-time profile selection flags and should not own profile
editing semantics.

`witty-app` should route profile-management commands before window/web/smoke
execution. The command handler can call:

- `default_profile_store_path()`
- `read_profile_store()`
- `edit_profile_store()`
- `ProfileStoreV1` mutation helpers

If profile-management flags are mixed with window mode, smoke modes, wasm
plugin startup flags, or launcher-only flags, parsing should fail.

## Test Plan

Focused tests for the first implementation slice:

- parse each profile-management mode
- reject missing values and invalid combinations
- list with an explicit store prints ids/names/tags/launchability but not host,
  user, identity path, config path, extra args, or remote command
- list with missing default store prints an empty list
- list with missing explicit store fails
- add creates a missing default or explicit store
- add with `--set-default` changes the default to the added profile
- update modifies an existing profile and rejects missing ids
- remove deletes an existing profile and clears default only when needed
- set-default and clear-default mutate only default state
- existing lock makes mutation fail and preserves the store

Verification should include:

```text
cargo fmt --check
cargo test -p witty-app profile_store_cli --quiet
cargo test -p witty-transport profile_store --quiet
cargo test -p witty-launcher profile_store --quiet
cargo check -p witty-web --target wasm32-unknown-unknown
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

## Implementation Status

`m232-profile-store-cli-list-add` implemented the first half of the planned
surface in `witty-app`:

- `witty --profile-store-list [--profile-store <path>]`
- `witty --profile-store-add --ssh-profile-json <path> [--profile-store
  <path>] [--set-default]`

List output is redacted TSV with default marker, id, name, launchability, and
tags. It does not print host, user, port, jump host, identity-file path,
config-file path, OpenSSH argv, remote command, credential secret id, or
serialized JSON.

Add uses `edit_profile_store(..., CreateIfMissing, ...)` so the sibling lock is
held across load, mutation, validation, and atomic replace. The mutation output
uses only non-sensitive report fields.

The launcher remains unchanged: `witty-launcher` still owns only `--web`
launch-time profile selection and does not own profile editing.

`m233-profile-store-cli-update-remove-default` completed the basic native CLI
mutation surface:

- `witty --profile-store-update --ssh-profile-json <path>
  [--profile-store <path>]`
- `witty --profile-store-remove <id> [--profile-store <path>]`
- `witty --profile-store-set-default <id> [--profile-store <path>]`
- `witty --profile-store-clear-default [--profile-store <path>]`

All four commands use `edit_profile_store(..., Existing, ...)`, so they fail on
missing stores and reuse the same locked read-modify-write path as add.

`m326-profile-store-launch-check` added a non-graphical read-only validation
command:

```text
witty --profile-store-check-launch <id> [--profile-store <path>]
```

It reads the same profile store boundary, checks that the selected id exists
and is launchable, validates conversion through `SshProfile::to_openssh_profile()`,
and prints only redacted booleans/counts such as default marker, `request_tty`,
TERM presence, credential/config/jump-host presence, and OpenSSH arg counts. It
does not start SSH or print host/user/path/argv/remote-command values. See
`profile-store-launch-check.md`.

## Follow-Up Queue

1. `m232-profile-store-cli-list-add`
   - done. Implemented native `witty-app` profile-management parser shell plus
     redacted `list` and locked `add`.
2. `m233-profile-store-cli-update-remove-default`
   - done. Implemented update, remove, set-default, and clear-default commands
     on the same native CLI surface.
3. `m234-profile-store-openssh-import-preview-plan`
   - done. See `profile-store-openssh-import-preview-plan.md`; designed the
     preview/candidate/warning/conflict boundary and deferred any write behavior
     to a later confirmed-import slice.
4. `m235-openssh-import-preview-types`
   - done. Added pure import preview/candidate/source/warning/conflict types
     and focused tests in `witty-transport`.
5. `m236-openssh-import-parser-subset`
   - done. Added pure conservative host-block parsing into import preview
     candidates without filesystem reads or profile-store writes.
6. `m237-profile-store-import-preview-cli`
   - done. Added native redacted preview output and optional explicit-store
     conflict detection.
7. `m238-profile-store-import-confirmed-write-plan`
   - done. Added the confirmed-write plan for explicit `--confirm`, selected
     candidate ids, `reject|replace` conflict policy, aggregate redacted
     output, and locked batch writes through `edit_profile_store()`.

## Non-Goals

- implicit or silent OpenSSH config import writes
- browser profile editor
- plugin profile write APIs
- vault/keychain resolver
- stale lock cleanup
- encrypted or team-synced profile stores
