# Profile Store Atomic Write Plan

Updated: 2026-05-31

## Purpose

`m225-profile-store-atomic-write-plan` turns the V1 profile store write policy
from `profile-store-file-plan.md` into an implementation contract.

The profile store is secret-free, but it still contains private host inventory,
user names, identity-file paths, jump hosts, tags, and remote commands. Writes
therefore need the same failure behavior expected from credentials-adjacent
configuration:

- never truncate or partially overwrite a valid existing store
- validate and serialize before touching the target path
- keep temporary files in the target directory so replacement is same-filesystem
- use restrictive permissions for newly created config directories/files
- make crash recovery explicit and testable

## Placement

Keep the pure schema available on both native and wasm targets:

```text
ProfileStoreV1::from_json()
ProfileStoreV1::to_pretty_json()
ProfileStoreV1::validate()
```

Add native-only file helpers in `witty-transport` rather than in `witty-launcher`:

```rust
#[cfg(not(target_arch = "wasm32"))]
pub fn read_profile_store(path: &Path) -> Result<ProfileStoreV1>;

#[cfg(not(target_arch = "wasm32"))]
pub fn write_profile_store_atomic(
    path: &Path,
    store: &ProfileStoreV1,
) -> Result<ProfileStoreWriteReport>;
```

Rationale:

- `witty-transport` already owns `ProfileStoreV1` validation and launchability.
- `witty-launcher` should choose a profile, not own persistence semantics.
- wasm can still display/edit imported JSON later without filesystem APIs.

`ProfileStoreWriteReport` should be small and non-sensitive:

```rust
pub struct ProfileStoreWriteReport {
    pub path: PathBuf,
    pub bytes_written: usize,
    pub created_parent_dir: bool,
}
```

Do not include profile ids, host names, user names, identity paths, argv, or
serialized JSON in the report.

## Write Algorithm

Target `write_profile_store_atomic(path, store)` behavior for m226:

1. Reject paths without a parent directory or usable file name.
2. Call `store.to_pretty_json()` before creating or opening any target/temp
   file. Validation failure must leave the filesystem untouched.
3. Ensure serialized byte length is still under
   `PROFILE_STORE_MAX_JSON_BYTES`.
4. Create the parent directory if missing.
5. On Unix, apply mode `0700` only to a directory created by this call. Do not
   chmod an existing directory behind the user's back.
6. Create a temporary file in the same parent directory. Use a hidden filename
   derived from the final basename plus process id and a retry counter, for
   example `.profiles.v1.json.tmp.<pid>.<counter>`.
7. Open the temp file with create-new semantics. Retry on temp-name collision;
   fail loudly after a small bounded number of collisions.
8. On Unix, create the temp file with mode `0600`.
9. Write the full JSON bytes, flush, and `sync_all()` the temp file.
10. Atomically replace the target with the temp file.
11. Best-effort `sync_all()` the parent directory on Unix.
12. If any step before replacement fails, best-effort remove only the temp file
    created by this call and leave the target untouched.

## Platform Boundary

For the Linux/macOS implementation path, same-directory `rename` gives the
needed atomic replacement behavior.

Windows has different replacement semantics when the destination already
exists. Do not fake atomic replacement with remove-then-rename. The
implementation should either:

- use a platform-specific replace primitive, or
- return an explicit unsupported error for replacement on Windows until that
  adapter is added.

The first product-quality implementation should make this boundary visible in
tests and docs instead of silently weakening atomicity.

## Implementation Status

`m226-profile-store-atomic-write-implementation` implemented the native helper
boundary in `witty-transport`:

- `read_profile_store(path)`
- `write_profile_store_atomic(path, &store)`
- `ProfileStoreWriteReport`

The implementation validates and serializes before creating the parent
directory or temp file, writes a hidden same-directory temp file with create-new
semantics, flushes and syncs it, then replaces the target. On Unix, newly
created parent directories are set to `0700` and newly created store files are
set to `0600`. On Windows, replacing an existing target remains an explicit
unsupported path until a platform-specific atomic replace adapter is added.

`witty-launcher` now uses `read_profile_store()` for
`--profile-store <path> --ssh-profile-id <id>` selection.

`m227-profile-store-default-path` added platform default profile store path
resolution and launcher default-store selection.

`m228-profile-store-locking` added a write-only sibling lock file to
`write_profile_store_atomic()`.

## Locking Boundary

The current write helper creates a sibling lock file before writing the
temporary store:

- lock name: target basename plus `.lock`, for example
  `profiles.v1.json.lock`
- acquisition: `create_new` only; existing locks fail loudly
- timing: after validation and parent directory creation, before temp file
  creation
- permissions: mode `0600` on Unix
- cleanup: the guard removes only a lock marker that matches the file it
  created
- stale locks: no automatic cleanup yet
- read path: not locked

This is a single-writer guard for upcoming profile edit/import helpers. It is
not a merge protocol.

Do not conflate atomic replacement with concurrent edit merging. Atomic write
prevents torn files; it does not resolve two writers editing different
profiles.

## Tests

Focused tests for m226:

- invalid store fails before creating the parent directory
- valid store creates missing parent and writes loadable JSON
- existing valid store remains unchanged when the new store fails validation
- replacement writes loadable new content without leaving normal temp files
- temp-file collision retries use create-new semantics
- Unix-only: newly created parent directory mode is `0700`
- Unix-only: newly created store file mode is `0600`
- launcher read path can use `read_profile_store()` without changing profile
  selection behavior

Additional focused tests for m228:

- invalid stores fail before creating a lock file
- existing lock makes the write fail and preserves the target
- successful writes remove their lock file
- replacing or modifying the lock marker prevents the guard from deleting the
  new file
- Unix-only: newly created lock file mode is `0600`

`cargo test -p witty-transport profile_store --quiet` should cover the file
helpers. `cargo test -p witty-launcher profile_store --quiet` should keep the
launcher selection regression.

## Follow-Up Queue

1. `m226-profile-store-atomic-write-implementation`
   - done. Added native-only helpers, launcher read migration, and focused
     permission/replacement tests.
2. `m227-profile-store-default-path`
   - done. Added platform config directory resolution and opt-in default
     launcher path behavior.
3. `m228-profile-store-locking`
   - done. Added write-only sibling lock acquisition, failure behavior, cleanup,
     and Unix permission tests.
4. `m229-profile-store-edit-api-plan`
   - done. See `profile-store-edit-api-plan.md`; read-modify-write edit helpers
     must hold the sibling lock across the read, mutation, validation, and
     atomic replace instead of reading before lock acquisition.
5. `m230-profile-store-edit-helpers`
   - done. Implemented pure store mutations plus a locked native edit
     transaction that reuses temp-file replacement without acquiring a second
     lock.
6. `m231-profile-store-cli-or-import-plan`
   - done. See `profile-store-cli-plan.md`; selected native CLI
     profile-management commands as the first product surface that will exercise
     locked edit transactions.
7. `m232-profile-store-cli-list-add`
   - done. Implemented redacted list and locked add commands in `witty-app`.
8. `m233-profile-store-cli-update-remove-default`
   - done. Implemented the remaining locked CLI mutations.
9. `m234-profile-store-openssh-import-preview-plan`
   - done. See `profile-store-openssh-import-preview-plan.md`; selected a
     preview-only OpenSSH config import first, with any later writes required to
     go through `edit_profile_store(..., CreateIfMissing, ...)`.
10. `m235-openssh-import-preview-types`
   - done. Added pure import preview/candidate/source/warning/conflict types.
11. `m236-openssh-import-parser-subset`
   - done. Added pure conservative host-block parsing without filesystem reads
     or writes.
12. `m237-profile-store-import-preview-cli`
   - add native redacted preview output and optional conflict detection.

## Non-Goals

- encrypted profile store
- keychain/vault resolver
- profile editing UI
- team sync or merge behavior
- OpenSSH config import
- SFTP/tunnel data models
