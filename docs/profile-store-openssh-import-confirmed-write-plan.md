# Profile Store OpenSSH Import Confirmed Write Plan

Updated: 2026-06-01

## Purpose

`m238-profile-store-import-confirmed-write-plan` defines the write side of the
two-phase OpenSSH config import flow.

The already implemented preview path can parse a user-supplied OpenSSH config,
produce `SshProfile` candidates, count warnings, and optionally mark
profile-id conflicts. This plan specifies how a later command may turn selected
preview candidates into `ProfileStoreV1` edits without adding silent imports,
auto-renames, plugin-owned writes, or browser filesystem access.

This is a plan only. It does not implement importer writes.

## Current Foundation

Already implemented:

- `SshProfile` product schema and launchability checks
- `ProfileStoreV1` validation and pure add/update/remove/default helpers
- native `edit_profile_store(path, mode, edit)` with read-modify-write locking
- native profile store CLI list/add/update/remove/default commands
- pure OpenSSH import preview types
- pure `parse_openssh_import_preview(config, config_path)`
- native redacted preview CLI:
  `witty --profile-store-import-openssh-preview <path> [--profile-store <path>]`

The write path must build on these pieces and keep the same privacy boundary as
preview.

## Command Shape

Add a separate confirmed write command:

```text
witty --profile-store-import-openssh <path> \
  --confirm \
  [--profile-store <path>] \
  [--conflict reject|replace] \
  [--import-profile-id <id>]...
```

Rules:

- `--confirm` is mandatory. Without it, the command fails and should direct the
  user to run the preview command first.
- `--profile-store <path>` selects the target store. When omitted, confirmed
  write uses `default_profile_store_path()`, matching add/list CLI behavior.
- `--conflict reject|replace` defaults to `reject`.
- `--import-profile-id <id>` is repeatable and selects candidates by preview
  candidate id. When omitted, all generated candidates are selected.
- The command must reject `--ssh-profile-json`, `--ssh-profile-id`,
  launcher-only flags, window-only flags, smoke modes, and other profile-store
  commands.

Do not add a browser route, plugin command, or UI confirmation surface in the
first confirmed-write implementation. Native CLI is the trusted owner.

## Selection Semantics

Selected candidates are identified by candidate profile id, not by host, line
number, name, or target host.

Before acquiring the profile-store lock, the command should:

1. Read only the supplied config path.
2. Parse a fresh `OpenSshImportPreview`.
3. Resolve the selection set.
4. Fail if any requested id is not present in the preview.
5. Fail if the selected candidate set contains duplicate profile ids.
6. Fail if no candidate is selected.

Warnings do not block import by default. The explicit `--confirm` means the
user accepts the preview result. The write report should still include
aggregate warning counts so automation can notice that the import was not
warning-free.

Global preview warnings for skipped wildcard/negated patterns are not imported.
They should contribute to the reported total warning count but should not block
selected concrete candidates.

## Conflict Policy

Use an explicit conflict policy enum in the implementation slice:

```rust
pub enum OpenSshImportConflictPolicy {
    Reject,
    Replace,
}
```

`reject`:

- If any selected candidate id already exists in the store, fail the whole
  import before mutating the in-memory store.
- Do not partially import non-conflicting candidates when any selected conflict
  exists.

`replace`:

- For each selected candidate whose id exists, replace that exact profile id.
- For each selected candidate whose id does not exist, add it.
- Do not auto-rename.
- Do not replace a different profile based on name, host, target, or source
  line.

Internal duplicate selected ids always fail, regardless of conflict policy.

## Store Edit Algorithm

Confirmed write must use:

```rust
edit_profile_store(path, ProfileStoreEditOpenMode::CreateIfMissing, |store| {
    // batch import mutation
})
```

The closure should perform a batch mutation from selected candidates:

1. Snapshot existing ids from `store`.
2. Compute conflicts between existing ids and selected candidate ids.
3. Apply the selected conflict policy.
4. For `replace`, update existing ids with `store.update_profile(id, profile)`.
5. For new ids, add profiles with `ProfileStoreDefaultPolicy::Preserve`.
6. Validate the full store through the existing helpers.
7. Return one aggregate `ProfileStoreMutation`.

Default profile behavior:

- Preserve `default_profile_id` by default.
- Replacing the current default profile keeps the same default id.
- Adding into an empty store does not automatically set a default in the first
  import write slice.
- A future explicit flag can set an imported default after the import surface
  has real user-facing confirmation.

The implementation may add a focused pure helper such as:

```rust
pub struct OpenSshImportApplyReport {
    pub selected: usize,
    pub added: usize,
    pub replaced: usize,
    pub warning_count: usize,
    pub global_warning_count: usize,
}
```

Keep this report aggregate-only. It should not contain hosts, users, identity
paths, config paths, remote commands, proxy commands, OpenSSH argv, or
serialized profile JSON.

## Output Contract

Suggested confirmed-write output:

```text
OpenSSH import applied: changed=true profiles=5 default_changed=false bytes=1234 created_parent_dir=false selected=3 added=2 replaced=1 warnings=4
```

Do not print:

- target hosts
- user names
- local identity-file paths
- local config paths
- `RemoteCommand`
- `ProxyCommand`
- extra argv
- serialized `SshProfile` or `ProfileStoreV1` JSON

Profile ids are already visible in preview/list output, but the default
confirmed-write mutation report should stay aggregate-only. A later local
verbose flag can expose ids if needed.

## Failure Behavior

Failure must leave the target store untouched.

Expected failures:

- missing `--confirm`
- missing or unreadable config file
- invalid `--conflict` value
- unknown selected `--import-profile-id`
- duplicate selected candidate id
- zero selected candidates
- selected candidate conflicts under `reject`
- missing or invalid existing target store when path exists but JSON is invalid
- existing profile-store lock
- final store validation failure

Use the existing `edit_profile_store()` transaction so read, mutation,
validation, and atomic replace all happen under the sibling write lock.

## Test Plan

Focused tests for the first implementation slice:

- parse the confirmed write command with explicit `--confirm`
- reject missing `--confirm`
- reject invalid `--conflict`
- reject invalid combinations with launcher/window/smoke/profile-store flags
- import all candidates into a missing default or explicit store with
  `CreateIfMissing`
- `reject` fails on an existing id and leaves the store bytes unchanged
- `replace` updates only exact conflicting ids and adds non-conflicting ids
- repeated `--import-profile-id` selects only requested ids
- unknown selected id fails before touching the store
- duplicate selected candidate ids fail before touching the store
- replacing the default profile preserves `default_profile_id`
- adding into an empty store preserves `default_profile_id: None`
- output contains aggregate counts but not host, user, identity path, config
  path, remote command, proxy command, or serialized JSON
- existing lock makes the import fail and preserves the store

Verification should include:

```text
cargo fmt --check
cargo test -p witty-app profile_store_import --quiet
cargo test -p witty-app profile_store_cli --quiet
cargo test -p witty-transport openssh_import --quiet
cargo test -p witty-transport profile_store --quiet
cargo check -p witty-web --target wasm32-unknown-unknown
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

## Follow-Up Queue

1. `m239-profile-store-import-confirmed-write-types`
   - done. Added `OpenSshImportConflictPolicy`,
     `OpenSshImportSelection`, `OpenSshImportApplyReport`, and
     `apply_openssh_import_preview(...)` in `witty-transport`, with pure
     selected-candidate batch application and no CLI write command.
2. `m240-profile-store-import-confirmed-write-cli`
   - done. Added the native confirmed write command, aggregate redacted output,
     and focused tests.
3. `m241-profile-store-import-confirmed-write-review`
   - done. Reviewed privacy, transaction safety, CLI combinations, and test
     coverage before considering any UI or plugin import surface.

## Non-Goals

- importing implicit `~/.ssh/config`
- recursive `Include` expansion
- importing `ProxyCommand`
- resolving `%`, environment, or `~` token expansion
- reading private key contents, passphrases, agents, or keychains
- browser-side config-file reading
- plugin-owned import writes
- silent auto-renaming of conflicting profiles
- setting an imported default profile in the first confirmed-write slice

## m239 Implementation Status

`m239-profile-store-import-confirmed-write-types` added the pure apply boundary
for the future native CLI:

- `OpenSshImportConflictPolicy::{Reject, Replace}` with CLI value parsing
- `OpenSshImportSelection::{All, ProfileIds(Vec<String>)}`
- `OpenSshImportApplyReport` with aggregate selected/added/replaced/warning
  counts and the existing `ProfileStoreMutation`
- `apply_openssh_import_preview(store, preview, selection, conflict_policy)`

The helper selects candidates by preview candidate profile id, rejects unknown
requested ids, rejects duplicate requested ids, rejects duplicate selected
candidate ids, rejects empty selections, preserves the target store on `reject`
conflicts, and under `replace` only updates exact matching ids while adding
new selected ids.

The helper uses `ProfileStoreDefaultPolicy::Preserve` for new imports, so
adding into an empty store does not silently choose a default. Replacing the
current default profile preserves the same default id.

Focused tests cover conflict-policy parsing, all-candidate add, reject
conflict preservation, exact-id replacement, selected-id filtering, unknown and
duplicate selection errors, duplicate candidate ids, default preservation,
warning counts, wasm compilation, workspace tests, and clippy.

## m240 Implementation Status

`m240-profile-store-import-confirmed-write-cli` added the native trusted CLI:

```text
witty --profile-store-import-openssh <path> \
  --confirm \
  [--profile-store <path>] \
  [--conflict reject|replace] \
  [--import-profile-id <id>]...
```

The command is intentionally separate from preview:

- `--confirm` is mandatory.
- `--conflict` defaults to `reject`; `replace` updates exact matching ids only.
- repeated `--import-profile-id` selects preview candidate ids; unknown,
  duplicate, or empty selections fail in the pure apply helper.
- confirmed writes resolve the default store path when `--profile-store` is
  omitted; preview still never resolves the default store path.
- writes use `edit_profile_store(..., CreateIfMissing, ...)` so the sibling
  lock covers read, mutation, validation, and atomic replace.
- output is aggregate-only: changed/profile/default/bytes/parent-dir plus
  selected/added/replaced/warning counts.

Focused CLI tests cover parse success, missing `--confirm`, invalid
`--conflict`, invalid combinations, all-candidate import into a missing store,
default-store import, selected-id import, reject conflict preservation,
exact-id replacement with default preservation, unknown/duplicate selected id
failures without writing store bytes, existing lock preservation, and output
redaction.

## m241 Review Status

`m241-profile-store-import-confirmed-write-review` found no blocking issues.

Reviewed boundaries:

- Privacy: successful confirmed-import output contains only aggregate
  changed/profile/default/bytes/parent-dir/selected/added/replaced/warning
  counts. Tests assert no host, user, identity path, config path, remote
  command, proxy command, or serialized JSON appears in success output.
- Transaction safety: confirmed writes call
  `edit_profile_store(..., CreateIfMissing, ...)`, so store read, mutation,
  validation, lock handling, and atomic replace stay under the native edit
  transaction. Reject conflicts, bad selections, and existing locks are covered
  by store-byte preservation tests.
- CLI combinations: `--confirm`, `--conflict`, and `--import-profile-id` are
  accepted only with `--profile-store-import-openssh`; profile-store commands
  reject launcher/window/smoke mixtures; preview remains non-writing and does
  not resolve the default store path.
- Scope: browser-side config-file reads, plugin-owned import writes, implicit
  `~/.ssh/config`, recursive `Include`, `ProxyCommand` import, and silent
  auto-renaming remain out of scope.

Residual risk: error messages may include local paths from trusted native file
I/O contexts, consistent with the existing CLI read/write errors. Success
output remains redacted.

Next recommended slice: plan a trusted host-owned profile-store command/UI
boundary before exposing profile selection or OpenSSH import workflows outside
the native CLI.
