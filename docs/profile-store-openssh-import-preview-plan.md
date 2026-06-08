# Profile Store OpenSSH Import Preview Plan

Updated: 2026-06-01

## Purpose

`m234-profile-store-openssh-import-preview-plan` defines the import boundary
after the native profile-store CLI gained list/add/update/remove/default
commands.

The goal is a preview-and-confirm OpenSSH config import flow. The first
implementation should parse candidates and warnings, show a non-writing
preview, and require an explicit later write command or confirmation step
before mutating `ProfileStoreV1`.

This is not a full OpenSSH policy engine, a vault resolver, or a team inventory
sync feature.

## Decision

Implement import as two phases:

1. Preview: read OpenSSH config input and emit candidate `SshProfile` records
   plus warnings and conflicts.
2. Confirmed write: take selected candidates and an explicit conflict policy,
   then apply them through `edit_profile_store()`.

The first implementation slice should stop at phase 1. This avoids silently
writing host inventory from a parser that still needs real-world config
coverage.

## Data Model

Add pure, non-filesystem import types in `witty-transport`:

```rust
pub struct OpenSshImportPreview {
    pub candidates: Vec<OpenSshImportCandidate>,
    pub warnings: Vec<OpenSshImportWarning>,
}

pub struct OpenSshImportCandidate {
    pub profile: SshProfile,
    pub source: OpenSshImportSource,
    pub warnings: Vec<OpenSshImportWarning>,
    pub conflict: Option<OpenSshImportConflict>,
}

pub struct OpenSshImportSource {
    pub config_path: Option<PathBuf>,
    pub host_pattern: String,
    pub line: Option<usize>,
}

pub enum OpenSshImportConflict {
    ExistingProfileId { profile_id: String },
}

pub enum OpenSshImportWarning {
    UnsupportedDirective { keyword: String },
    UnsupportedPattern { pattern: String },
    InvalidDirectiveValue { keyword: String, value: String },
    MissingHostName { host_pattern: String },
    SkippedWildcardOnlyHost { pattern: String },
    EmptyProfileId { host_pattern: String },
    TokenExpansionPreserved { field: String },
    IdentityFilePathOnly,
    RemoteCommandImported,
    ProxyCommandNotImported,
    IncludeNotExpanded { path: String },
}
```

Warnings may evolve, but they must remain structured enough for CLI, UI, and
tests to filter without parsing prose.

## Supported OpenSSH Subset

Start with a conservative subset of host blocks:

- `Host`
- `HostName`
- `User`
- `Port`
- `IdentityFile`
- `ProxyJump`
- `RequestTTY`
- `SetEnv TERM=...` only when unambiguous
- `RemoteCommand`

Initial parser rules:

- Parse plain files supplied by the user, not implicit `~/.ssh/config`, unless
  the caller explicitly chooses that path.
- Treat `Include` as a warning in the first parser slice. Recursive include
  traversal can follow after path safety, cycle detection, and test fixtures
  exist.
- Skip wildcard-only or negated `Host` patterns by default because they do not
  map cleanly to one launchable product profile.
- For multi-pattern `Host` lines, create candidates only for concrete patterns
  and warn about skipped wildcard/negated patterns.
- Prefer `HostName` as `target.host`; fall back to concrete `Host` alias when
  `HostName` is missing and warn.
- Preserve `IdentityFile` as a path reference only. Never read private key
  contents or passphrases.
- Do not import `ProxyCommand` into `extra_args` initially; warn instead. It is
  too broad and can execute arbitrary local commands.
- Do not resolve OpenSSH `%h`, `%n`, `%r`, `%p`, environment variables, or
  `~` in the first parser slice. Preserve the literal path/value where the
  `SshProfile` schema allows it and attach token-expansion warnings.

## Profile ID And Metadata Rules

Candidate id:

- derive from the concrete `Host` pattern
- lowercase ASCII where possible
- replace unsupported profile-id characters with `-`
- collapse repeated separators
- trim leading/trailing separators
- if empty, skip the candidate with a warning

Candidate name:

- use the original concrete `Host` alias when present
- otherwise use `HostName`

Tags:

- add `imported`
- add `openssh`

Description:

- optional, for example `Imported from OpenSSH config`
- do not include local absolute config path by default in the profile

## Preview CLI Shape

Add a preview command before write commands:

```text
witty --profile-store-import-openssh-preview <path> [--profile-store <path>]
```

Behavior:

- reads only the supplied OpenSSH config path
- optionally reads the selected profile store to detect id conflicts
- prints redacted TSV or JSON-like summary:
  - candidate id
  - name
  - launchability
  - conflict status
  - warning count
- does not print host/user/identity/config path/remote command by default
- does not write the profile store

The `--profile-store <path>` flag is optional for preview. When omitted, the
preview should still parse candidates but skip conflict detection or use the
default store only if the command explicitly says so in a later slice.

## Confirmed Write Shape

Do not implement in the preview slice, but reserve explicit commands such as:

```text
witty --profile-store-import-openssh <path> \
  --confirm \
  [--profile-store <path>] \
  [--conflict reject|replace]
```

Initial conflict policy:

- `reject`: fail if any selected candidate id already exists
- `replace`: replace only exact selected conflicts

Do not silently auto-rename in the first write implementation. Auto-renaming is
hard to make predictable and can hide duplicate host inventory.

Confirmed writes must use `edit_profile_store(..., CreateIfMissing, ...)` and
the existing pure mutation helpers. Import batches should validate the complete
store before writing and return only non-sensitive summary fields.

## Privacy Boundary

Import preview can process private host inventory, but output should default to
redacted fields. Raw details may be available later behind an explicit verbose
flag for local CLI use, but the default should not print:

- host names
- user names
- local identity-file paths
- local config paths
- `RemoteCommand`
- `ProxyCommand`
- extra argv

Plugins should not receive import candidates by default. A future plugin API
should request import through a host-owned command and display a host-rendered
confirmation surface.

## Test Plan

Parser/preview tests for the first implementation slice:

- simple `Host prod` with `HostName`, `User`, `Port`, `IdentityFile`,
  `ProxyJump`, and `RemoteCommand` produces one candidate and path-only
  credential reference
- `Host *` is skipped with a wildcard warning
- negated patterns are skipped with a warning
- missing `HostName` falls back to concrete alias with a warning
- `ProxyCommand` is not imported and produces a warning
- `Include` is not expanded and produces a warning
- conflict detection marks existing ids without mutating the store
- preview output does not contain host, user, identity path, remote command, or
  local config path by default

Verification should include:

```text
cargo fmt --check
cargo test -p witty-transport openssh_import --quiet
cargo test -p witty-app profile_store_import --quiet
cargo test -p witty-app profile_store_cli --quiet
cargo check -p witty-web --target wasm32-unknown-unknown
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

## Follow-Up Queue

1. `m235-openssh-import-preview-types`
   - done. Added pure `OpenSshImportPreview`, candidate, source, warning, and
     conflict types in `witty-transport`, plus focused serialization/counting
     tests and a pure `mark_conflicts_from_store()` helper.
2. `m236-openssh-import-parser-subset`
   - done. Added `parse_openssh_import_preview(config, config_path)` for the
     conservative host-block subset without filesystem reads or writes.
3. `m237-profile-store-import-preview-cli`
   - done. Added native `witty --profile-store-import-openssh-preview
     <path> [--profile-store <path>]` with redacted output and optional
     explicit-store conflict detection, while keeping preview non-writing.
4. `m238-profile-store-import-confirmed-write-plan`
   - done. Added `profile-store-openssh-import-confirmed-write-plan.md`,
     defining explicit `--confirm`, selection by candidate id, `reject|replace`
     conflict policy, aggregate redacted output, and locked
     `edit_profile_store(..., CreateIfMissing, ...)` write semantics before any
     importer write implementation.

## Non-Goals

- implicit import from `~/.ssh/config`
- recursive `Include` expansion
- `ProxyCommand` import
- resolving OpenSSH token expansion or environment expansion
- reading private key contents, passphrases, agents, or keychains
- browser-side config-file reading
- plugin-owned profile import
- writing imported candidates in the preview slice

## m235 Implementation Status

`m235-openssh-import-preview-types` implemented the pure preview data boundary:

- `OpenSshImportPreview`
- `OpenSshImportCandidate`
- `OpenSshImportSource`
- `OpenSshImportWarning`
- `OpenSshImportConflict`

The types derive serde traits for native and wasm consumers, keep conflict
status separate from warnings, and include `OpenSshImportPreview` helpers for
total warning count, conflict count, and pure conflict marking from an existing
`ProfileStoreV1`.

No parser, CLI command, filesystem read, or profile-store write behavior was
added in m235.

## m236 Implementation Status

`m236-openssh-import-parser-subset` added a pure parser entry point:

```rust
pub fn parse_openssh_import_preview(
    config: &str,
    config_path: Option<PathBuf>,
) -> OpenSshImportPreview
```

The parser recognizes the conservative first subset:

- `Host`
- `HostName`
- `User`
- `Port`
- `IdentityFile`
- `ProxyJump`
- `RequestTTY`
- `SetEnv TERM=...` when unambiguous
- `RemoteCommand`

It does not read the filesystem, implicitly load `~/.ssh/config`, expand
`Include`, resolve `%` tokens, expand environment variables, expand `~`, import
`ProxyCommand`, or write `ProfileStoreV1`.

Focused tests cover full candidate construction, wildcard/negated pattern
skips, missing `HostName` fallback, `Include` warnings, `ProxyCommand`
warnings, unsupported directives, invalid `Port`, token-preservation warnings,
serde shape, wasm compilation, workspace tests, and clippy.

## m237 Implementation Status

`m237-profile-store-import-preview-cli` added the native preview command:

```text
witty --profile-store-import-openssh-preview <path> [--profile-store <path>]
```

The command reads only the supplied OpenSSH config path, calls the pure
`parse_openssh_import_preview(config, Some(path))`, and prints a redacted
TSV-like summary with:

- candidate id
- candidate name
- launchability label
- conflict label
- candidate warning count

It appends a non-sensitive aggregate summary with candidate, conflict, and total
warning counts. Default output does not include target hosts, users, identity
file paths, local config paths, remote commands, proxy commands, or extra argv.

Conflict detection is intentionally opt-in: it runs only when `--profile-store
<path>` is supplied. Without an explicit profile store, preview does not resolve
or read the default profile store path. With an explicit missing store, it fails
through the existing `read_profile_store()` path.

No `ProfileStoreV1` writes occur in the preview slice. Focused tests cover CLI
parsing, invalid combinations, redaction, skipped default-store resolution,
explicit-store conflict marking, and explicit missing-store failure. Verification
also covered formatter check, transport parser tests, wasm check, workspace
tests, clippy, and diff whitespace checks.

## m238 Planning Status

`m238-profile-store-import-confirmed-write-plan` added
`profile-store-openssh-import-confirmed-write-plan.md`.

The planned confirmed-write surface is:

```text
witty --profile-store-import-openssh <path> \
  --confirm \
  [--profile-store <path>] \
  [--conflict reject|replace] \
  [--import-profile-id <id>]...
```

The plan keeps writes native-only and explicit, selects candidates by preview
candidate id, defaults to `reject` conflicts, allows exact-id `replace`, rejects
duplicate selected ids, preserves defaults by default, and keeps output
aggregate-only and redacted.
