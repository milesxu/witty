# Profile Store File Plan

Updated: 2026-05-31

## Purpose

`m222-profile-store-file-plan` defines the local SSH profile store boundary that
should replace the temporary `witty --web --ssh-profile-json <path>` entry.

The goal is a durable, versioned local file store for profile metadata and
credential references, while keeping secret material out of profile JSON.

## Store Location

Use platform config directories, not the project working directory:

| Platform | Default Store |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/witty/profiles.v1.json`, falling back to `~/.config/witty/profiles.v1.json` |
| macOS | `~/Library/Application Support/Witty/profiles.v1.json` |
| Windows | `%APPDATA%\\Witty\\profiles.v1.json` |

Implementation can start with explicit `--profile-store <path>` for tests and
developer builds. Product defaults should use platform directories once a small
config-dir helper or dependency is selected.

`m227-profile-store-default-path` implemented default path resolution without a
new dependency:

- Linux and other non-macOS Unix: `$XDG_CONFIG_HOME/witty/profiles.v1.json`
  when `XDG_CONFIG_HOME` is an absolute path, otherwise
  `$HOME/.config/witty/profiles.v1.json`
- macOS: `~/Library/Application Support/Witty/profiles.v1.json`
- Windows: `%APPDATA%\\Witty\\profiles.v1.json`

The launcher now treats `--ssh-profile-id <id>` without `--profile-store` as an
explicit request to read the default profile store. `--profile-store <path>`
still overrides the default and still requires `--ssh-profile-id`.

## File Schema V1

Top-level store:

```json
{
  "schema": 1,
  "app": "witty-profiles",
  "profiles": [],
  "default_profile_id": null
}
```

Each `profiles[]` entry is the existing `witty_transport::SshProfile` schema:

- `id`, `name`, `description`, `tags`
- `target`: `host`, `user`, `port`, `jump_host`
- `credential`: `default_agent`, `identity_file`, or `vault_secret`
- `terminal`: `term`, `request_tty`
- `openssh`: `config_file`, `extra_args`, `remote_command`

V1 deliberately does not include folders, teams, history, SFTP bookmarks,
tunnels, AI prompts, or per-host runtime state. Those should become explicit
versioned fields later instead of being hidden in free-form blobs.

`m223-profile-store-types` implemented this as `witty_transport::ProfileStoreV1`
with exported constants for the schema/app string and conservative limits.
The type is pure data plus JSON validation and is available to both native and
wasm consumers. It does not read default platform paths or perform file writes.

## Secret Boundary

The store may contain references:

- `default_agent`: no secret field
- `identity_file.path`: path reference only
- `vault_secret.secret_id`: opaque id for a future resolver

The store must not contain:

- private key bytes
- passphrases
- decrypted vault values
- SSH agent protocol blobs
- one-time tokens
- known-host trust overrides

Future keychain/vault integration should introduce a resolver interface that
maps `SshCredentialRef` to launch-time behavior. The current `SshProfile`
conversion should keep rejecting unresolved `vault_secret` references until
that resolver exists.

## Validation

Loading the store should fail loudly when:

- `schema` is missing or unsupported
- `app` is not `witty-profiles`
- profile ids are empty, unsafe, or duplicated
- `default_profile_id` does not reference an existing profile
- any profile fails the existing `SshProfile::to_openssh_profile()` validation,
  except unresolved `vault_secret` profiles may be retained as non-launchable
  records
- file size or profile count exceeds conservative limits

Suggested initial limits:

- maximum store file size: 1 MiB
- maximum profiles: 512
- maximum tags per profile: 32
- maximum OpenSSH `extra_args` entries: 64
- maximum remote command entries: 64

These limits are product guardrails, not protocol limitations.

The implemented validation returns a `ProfileStoreValidation` summary with:

- launchable profile count
- credential-resolver-required profile count

This lets the store retain unresolved `vault_secret` profiles for display and
future resolver-backed launch without pretending they are currently launchable.

## Write Policy

Profile store writes should be local and atomic:

1. Create the parent directory if needed.
2. On Unix, set directory permissions to `0700` when creating it.
3. Serialize canonical pretty JSON.
4. Write to a temporary file in the same directory.
5. On Unix, set file permissions to `0600`.
6. Flush the file and rename it over the target.
7. Best effort fsync the parent directory.

Use a simple lock file or single-process writer guard before building richer
multi-window profile editing. A corrupted or partially written store should
produce an explicit error and leave the existing file untouched.

`m225-profile-store-atomic-write-plan` expands this into the implementation
contract in `profile-store-atomic-write-plan.md`. The selected next slice is a
native-only helper in `witty-transport` that validates/serializes before
touching the target path, writes a restrictive same-directory temporary file,
flushes it, atomically replaces the target where the platform supports that
semantics, and leaves multi-writer locking as a separate follow-up.

`m226-profile-store-atomic-write-implementation` implemented the native helper
boundary as `read_profile_store()` and `write_profile_store_atomic()` in
`witty-transport`, plus `ProfileStoreWriteReport`. The launcher now uses the
shared reader for `--profile-store <path> --ssh-profile-id <id>`.

`m227-profile-store-default-path` added platform default profile store path
resolution and launcher support for `--ssh-profile-id <id>` without an explicit
`--profile-store`.

`m228-profile-store-locking` added a conservative sibling write lock to
`write_profile_store_atomic()`. The lock file is named after the target store,
for example `profiles.v1.json.lock`, is opened with create-new semantics, and
uses mode `0600` on Unix. The lock is acquired only after validation and parent
directory creation, so invalid stores still leave the filesystem untouched. A
pre-existing lock fails the write and is not removed. Witty does not attempt
stale lock cleanup or concurrent edit merging in this slice.

`m229-profile-store-edit-api-plan` added
`profile-store-edit-api-plan.md`, selecting pure `ProfileStoreV1` mutation
methods plus a native `edit_profile_store()` read-modify-write transaction that
holds the sibling lock across read, mutation, validation, and atomic replace.

## Migration Policy

Readers should accept only known schema versions.

V1 rules:

- `schema == 1`: load directly
- missing schema: fail with a migration-required error
- higher schema: fail with an unsupported-version error

When V2 is introduced, add a pure migration function:

```text
ProfileStoreV1 -> ProfileStoreV2
```

Migration should run in memory first, validate the output, then write the new
file atomically. Do not silently discard unknown future fields.

## Launcher Flow

The next product launch path should be:

```text
witty --web --profile-store <path> --ssh-profile-id prod
  -> load ProfileStoreV1
  -> select profile by id
  -> convert to OpenSshProfile
  -> convert to LocalPtyConfig
  -> pass trusted PTY config to witty-gateway
```

`m224-profile-store-launcher-selection` implemented this explicit store path in
trusted native launcher code. The launcher accepts the two flags in either
order, requires both together, rejects missing profile ids, and rejects
`vault_secret` profiles until a credential resolver exists. Raw `--program` and
`--arg` remain mutually exclusive with SSH profile launch.

The browser session config remains unchanged and must not include profile
metadata, selected id, host, user, identity-file path, or argv.

`--ssh-profile-json <path>` remains as a developer/testing escape hatch for now.

## Plugin And Sync Boundary

Plugins should not receive raw profile store contents by default.

Future plugin APIs can expose narrow commands such as:

- request a profile launch by id
- request a redacted profile list
- request profile import with user confirmation

They should not expose credential references, local identity paths, OpenSSH
extra args, or remote commands without an explicit permission and UI prompt.

Team sync should treat this V1 local store as a private local source of truth.
If sync is added later, it should sync redacted metadata by default and keep
credential references local or explicitly mapped per machine.

## Follow-Up Tasks

1. `m225-profile-store-atomic-write-plan`
   - done. See `profile-store-atomic-write-plan.md`.
2. `m226-profile-store-atomic-write-implementation`
   - done. Added native-only read/write helpers in `witty-transport`, migrated
     launcher store reads to the shared reader, and verified
     permissions/replacement behavior.
3. `m227-profile-store-default-path`
   - done. Added platform config directory resolution and opt-in default
     launcher path behavior.
4. `m228-profile-store-locking`
   - done. Added write-only sibling lock behavior before profile editing UI/API
     work.
5. `m229-profile-store-edit-api-plan`
   - done. Planned pure add/update/delete/default mutation helpers, a locked
     native transaction wrapper, import boundaries, plugin/UI ownership, and
     m230 implementation scope.
6. `m230-profile-store-edit-helpers`
   - done. Implemented pure `ProfileStoreV1` add/update/remove/default
     mutations, native `edit_profile_store()`, locked transaction behavior, and
     focused tests.
7. `m231-profile-store-cli-or-import-plan`
   - done. See `profile-store-cli-plan.md`; selected native CLI profile
     management as the first product surface for locked edit helpers, with
     OpenSSH config import deferred to a later preview-and-confirm flow.
8. `m232-profile-store-cli-list-add`
   - done. Implemented the native `witty-app` profile-management parser shell
     plus redacted list and locked add commands.
9. `m233-profile-store-cli-update-remove-default`
   - done. Implemented update, remove, set-default, and clear-default commands.
10. `m234-profile-store-openssh-import-preview-plan`
   - done. See `profile-store-openssh-import-preview-plan.md`; designed the
     OpenSSH config import preview/candidate/warning/conflict boundary and
     deferred all writes to a later confirmed-import slice.
11. `m235-openssh-import-preview-types`
   - done. Added pure import preview/candidate/source/warning/conflict types
     before parsing real OpenSSH configs.
12. `m236-openssh-import-parser-subset`
   - done. Added pure conservative host-block parsing without filesystem reads
     or profile-store writes.
13. `m237-profile-store-import-preview-cli`
   - add native redacted preview output and optional conflict detection.

## Non-Goals

- encrypted vault implementation
- keychain integration
- OpenSSH config import UI
- team sync
- SFTP bookmarks
- tunnel dashboard
- browser-side profile editing
