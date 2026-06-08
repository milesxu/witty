# Profile Store Edit API Plan

Updated: 2026-05-31

## Purpose

`m229-profile-store-edit-api-plan` defines the next implementation slice after
schema validation, launcher selection, atomic writes, default paths, and write
locking.

The immediate goal is a small product-safe edit boundary for local SSH
profiles:

- add a profile
- update an existing profile
- remove a profile
- set or clear the default profile
- prepare for explicit import flows without adding UI yet

This is not a profile manager UI, an OpenSSH config importer, or a vault/team
sync feature.

## Current Foundation

Already implemented:

- `ProfileStoreV1` schema and validation
- `SshProfile` product schema and OpenSSH conversion
- `read_profile_store(path)`
- `write_profile_store_atomic(path, &store)`
- default platform store path resolution
- write-only sibling lock file during atomic writes
- `ProfileStoreDefaultPolicy`, `ProfileStoreMutation`, and pure
  `ProfileStoreV1` add/update/remove/default mutation helpers
- native-only `ProfileStoreEditOpenMode`, `ProfileStoreEditReport`, and
  `edit_profile_store(path, mode, edit)` transaction helper

The important m229 constraint: read-modify-write edits must hold the same
profile-store write lock across the read, mutation, validation, and atomic
replace. Calling `read_profile_store()`, editing in memory, and then calling
`write_profile_store_atomic()` would leave a race window where another writer
could update the file between the read and the lock acquisition.

## API Shape

Keep pure store mutation available on native and wasm targets:

```rust
impl ProfileStoreV1 {
    pub fn add_profile(
        &mut self,
        profile: SshProfile,
        default_policy: ProfileStoreDefaultPolicy,
    ) -> Result<ProfileStoreMutation>;

    pub fn update_profile(
        &mut self,
        id: &str,
        profile: SshProfile,
    ) -> Result<ProfileStoreMutation>;

    pub fn remove_profile(&mut self, id: &str) -> Result<ProfileStoreMutation>;

    pub fn set_default_profile(
        &mut self,
        id: Option<&str>,
    ) -> Result<ProfileStoreMutation>;
}
```

Native filesystem helpers can then compose these pure operations:

```rust
#[cfg(not(target_arch = "wasm32"))]
pub fn edit_profile_store(
    path: &Path,
    mode: ProfileStoreEditOpenMode,
    edit: impl FnOnce(&mut ProfileStoreV1) -> Result<ProfileStoreMutation>,
) -> Result<ProfileStoreEditReport>;
```

The transaction wrapper should:

1. Validate the target path and file name.
2. Create the parent directory only when needed for locking or create-if-missing
   mode.
3. Acquire the sibling lock before reading an existing store.
4. Load the existing store, or create `ProfileStoreV1::new()` when mode allows.
5. Apply the mutation.
6. Validate and serialize the final store.
7. Write via the same temporary-file and replace path used by
   `write_profile_store_atomic()`, without acquiring a second lock.
8. Release only the lock marker created by the current transaction.

`write_profile_store_atomic()` should keep its public behavior. Internally it
can share a lower-level locked write helper with `edit_profile_store()`.

## Edit Modes

Use an explicit open mode instead of silently creating stores:

```rust
pub enum ProfileStoreEditOpenMode {
    Existing,
    CreateIfMissing,
}
```

Rules:

- `Existing` fails if the store file does not exist.
- `CreateIfMissing` starts from an empty V1 store when the file does not exist.
- Invalid existing JSON fails before mutation and leaves the file untouched.
- Missing parent directory is created only when `CreateIfMissing` is used or
  when the target parent is needed for lock acquisition on an existing store
  path.

## Mutation Semantics

`add_profile`:

- rejects duplicate ids
- validates the inserted profile through whole-store validation
- if this is the first profile and no default exists, sets the added profile as
  default unless policy says otherwise

`update_profile`:

- requires the target id to exist
- initially rejects profile id changes; rename can be a later explicit API
- preserves `default_profile_id` when the same id remains valid
- validates the whole store after replacement

`remove_profile`:

- requires the target id to exist
- removes the profile
- clears `default_profile_id` if the deleted profile was default
- does not silently pick a new default

`set_default_profile`:

- accepts `None` to clear the default
- requires a non-`None` id to exist
- reuses existing id validation

## Default Policy

Start with a small policy enum:

```rust
pub enum ProfileStoreDefaultPolicy {
    Preserve,
    SetIfEmpty,
    SetToAdded,
}
```

Recommended command/UI behavior:

- manual "add profile": `SetIfEmpty`
- explicit "add and make default": `SetToAdded`
- import batch: `Preserve` unless the user explicitly chooses a default

## Reports

Reports should stay non-sensitive:

```rust
pub struct ProfileStoreMutation {
    pub changed: bool,
    pub profile_count: usize,
    pub default_profile_changed: bool,
}

pub struct ProfileStoreEditReport {
    pub write: ProfileStoreWriteReport,
    pub mutation: ProfileStoreMutation,
}
```

Do not include hosts, users, identity-file paths, remote commands, OpenSSH
extra args, credential references, serialized JSON, or profile names in reports.

Profile ids can still be sensitive inventory in team or plugin contexts, so
avoid returning them from generic reports. UI call sites already know the id
they requested.

## Import Boundary

Do not implement OpenSSH config import in m230. Reserve a later importer API
that returns candidate `SshProfile` values plus warnings:

```rust
pub struct ProfileImportCandidate {
    pub profile: SshProfile,
    pub warnings: Vec<ProfileImportWarning>,
}
```

The importer must not read private key contents or ask agents/keychains for
secrets. It may preserve identity-file paths as references only, subject to a
user confirmation screen before writing.

Conflict policy for future import batches should be explicit:

- reject conflicting ids
- replace selected conflicts
- import under user-selected new ids

Do not auto-rename silently in the first importer.

## Plugin And UI Boundary

Plugins should not get raw store editing by default.

Initial product commands should be host-owned:

- profile list with redacted metadata
- add/update/delete after user confirmation
- import preview after user confirmation
- launch by id through the existing trusted launcher path

Plugin APIs can later request these actions, but profile writes should require
host confirmation and should not expose credential references, local paths, or
OpenSSH argv to the plugin unless a dedicated permission exists.

## m230 Implementation Status

`m230-profile-store-edit-helpers` implemented the selected API shape:

- added pure store mutations on `ProfileStoreV1`
- added native `edit_profile_store(path, mode, edit)`
- split atomic write internals so locked edit transactions reuse the same
  temp-file replacement path without acquiring a second lock
- kept `write_profile_store_atomic()` public behavior unchanged
- kept launcher behavior unchanged
- covered pure mutation behavior, create-if-missing edits, existing-mode
  missing stores, existing lock conflicts, mutation failures, and invalid JSON
  preservation

`m231-profile-store-cli-or-import-plan` chose a native CLI profile-management
surface as the first product entry point for these helpers. See
`profile-store-cli-plan.md`.

`m232-profile-store-cli-list-add` and
`m233-profile-store-cli-update-remove-default` implemented the initial native
CLI profile-management surface on top of these helpers.

`m234-profile-store-openssh-import-preview-plan` added
`profile-store-openssh-import-preview-plan.md`, selecting a two-phase OpenSSH
config import flow: preview candidates and warnings first, then a later
explicit confirmed write path. The preview slice must not mutate
`ProfileStoreV1`.

`m235-openssh-import-preview-types` added the pure preview/candidate/source,
warning, and conflict types in `witty-transport`, plus focused serialization and
conflict-count tests. It did not add parsing, CLI, or filesystem writes.

`m236-openssh-import-parser-subset` added pure conservative OpenSSH host-block
parsing into `OpenSshImportPreview` without filesystem reads, implicit config
loading, or profile-store writes.

Next implementation candidates:

- add a redacted native CLI preview command with optional conflict detection
- add stale lock diagnostics only when a product surface exists for showing
  lock age/path/process metadata

## Verification

Checks used for m230:

- `cargo fmt --check`
- `cargo test -p witty-transport profile_store --quiet`
- `cargo test -p witty-launcher profile_store --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `git diff --check`

## Non-Goals

- profile manager UI
- OpenSSH config parser/importer implementation
- vault resolver
- encrypted profile store
- stale lock cleanup UI
- concurrent merge protocol
- team sync
