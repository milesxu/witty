# Profile Store Host-Owned Command Plan

Updated: 2026-06-01

## Purpose

`m242-profile-store-host-owned-command-plan` defines the next product boundary
after native profile-store CLI management and confirmed OpenSSH config import.

The goal is to make profile selection and later import workflows usable from a
trusted Witty UI without giving plugins, browser wasm, or terminal content
direct access to the raw `ProfileStoreV1` file.

## Current Foundation

Already implemented:

- `SshProfile` schema with credential references, not secret material
- `ProfileStoreV1` validation and launchability summaries
- default profile store path resolution
- trusted native launcher selection by profile id:
  `witty --web [--profile-store <path>] --ssh-profile-id <id>`
- native profile-store CLI list/add/update/remove/default commands
- non-writing OpenSSH import preview CLI
- confirmed OpenSSH import write CLI with mandatory `--confirm`
- locked `edit_profile_store(..., CreateIfMissing, ...)` mutation path
- aggregate redacted mutation output
- pure `ProfileStoreV1::redacted_summary()` helper with serializable
  `ProfileStoreSummary` / `ProfileSummary` UI-safe types
- `launcher-profile-picker-plan.md`, defining explicit
  `witty --web --profile-picker [--profile-store <path>]`, a separate
  picker UI token, redacted bootstrap route, selected-id handoff, and deferred
  gateway launch

## Boundary Decision

Profile-store UI commands must be host-owned.

Host-owned means:

- native Witty code reads and writes the profile store
- native Witty code converts a selected profile into an OpenSSH-backed
  `LocalPtyConfig`
- browser UI, wasm code, plugins, and terminal output do not receive raw
  `ProfileStoreV1`, credential references, local paths, OpenSSH argv, remote
  commands, or imported candidate profiles by default
- user confirmation is required before writes

This matches the existing command-block/search/clipboard privacy pattern: local
commands may operate on sensitive state, but plugin command invocation should
not receive sensitive payloads unless a dedicated permission model exists.

## Redacted Profile Summary

The first reusable UI primitive is a redacted profile summary:

```rust
pub struct ProfileStoreSummary {
    pub profiles: Vec<ProfileSummary>,
    pub default_profile_id: Option<String>,
    pub launchable_profiles: usize,
    pub credential_resolver_required_profiles: usize,
}

pub struct ProfileSummary {
    pub id: String,
    pub name: String,
    pub tags: Vec<String>,
    pub launchability: SshProfileLaunchability,
    pub is_default: bool,
}
```

It must not include:

- target host
- user
- port
- jump host
- identity-file path
- config-file path
- OpenSSH extra args
- remote command
- credential secret id
- serialized profile JSON

Profile ids and names are already visible in list/preview CLI output, but they
still represent inventory. They are acceptable for trusted host-rendered UI and
redacted browser shell UI, but should not be sent to plugins by default.

## Product Surfaces

### Native Window

Native window mode can eventually register builtin commands such as:

- `Profile Store: Open Profile Picker`
- `Profile Store: Launch Default`
- `Profile Store: Import OpenSSH Config`

The immediate implementation should not attempt live transport replacement in
the existing native terminal window. Native window currently owns a concrete
`TerminalApp<LocalPtyTransport>` path. Switching to an SSH profile in-place
requires a separate transport lifecycle design.

For now, native window commands should be planned as host-owned UI/actions, not
plugin APIs.

### Browser Product Launcher

The browser product path already has the right native trust boundary:

```text
witty --web
  -> native launcher owns local filesystem and gateway process
  -> browser receives one-use session config only
```

The first browser-facing profile UI should be a launcher-owned pre-session
profile picker:

1. Native launcher reads the selected/default profile store.
2. Native launcher serves only a redacted profile summary to the browser shell.
3. Browser displays profile id/name/tags/launchability/default marker.
4. User selects a profile.
5. Browser sends the selected id plus launcher UI token to native launcher.
6. Native launcher re-reads the store, validates the id, converts the profile,
   starts the gateway, and returns the existing one-use session config.

The browser must not receive target host, user, identity-file path, config path,
remote command, OpenSSH argv, or credential secret id.

OpenSSH import from browser UI should remain out of scope until a native file
picker or explicit user-selected file bridge exists. A browser text field that
accepts arbitrary local paths is not a safe import confirmation surface.

### Plugin Requests

Plugins should not receive profile summaries or import candidates by default.

A future plugin API may request host actions such as:

- `request_profile_picker`
- `request_profile_launch { id }`
- `request_openssh_import_preview`

The host must render the picker/confirmation UI and return only aggregate
success/failure status unless a dedicated permission grants inventory access.

`m480-plugin-profile-picker-request-action` and
`m482-plugin-profile-launch-request-action` implement the first two as
permission-gated queue-only actions. They do not render UI, launch SSH, or
return profile data yet.

## Security Rules

- Profile store reads/writes stay in native code.
- Writes always require user confirmation.
- Profile import writes continue to use the confirmed CLI/import transaction
  semantics unless and until a host-rendered confirmation UI reproduces them.
- Browser routes must be loopback-only and token-protected like existing
  launcher session config routes.
- Browser profile picker routes should use a separate short-lived UI token and
  should not reuse gateway tokens in URLs beyond the existing session handoff.
- Profile selection must re-read and validate the store at launch time instead
  of trusting browser-returned profile details.
- Plugin commands may open host-owned UI but must not receive raw profile data.
- Logs and diagnostics should use counts and status labels, not profile
  internals.

## Suggested Task Split

1. `m243-profile-store-redacted-summary-types`
   - done. Added pure redacted summary types/helpers in `witty-transport`
   - covered launchability/default/tag/name/id behavior
   - asserted no sensitive profile fields are present in summary serialization
   - kept native file I/O unchanged
2. `m244-launcher-profile-picker-plan`
   - done. Added `launcher-profile-picker-plan.md`, covering the explicit
     picker entry point, route shape, token model, browser state, failure
     handling, security rules, and product smoke strategy
3. `m245-launcher-profile-picker-redacted-api`
   - done. Added native launcher `--profile-picker` parsing, default/explicit
     profile-store path resolution, `ProfilePickerSession` bootstrap JSON with
     a separate UI token, and one-use `GET /profile-picker/<id>.json` serving
     only `ProfileStoreSummary`
   - kept gateway spawn and selected-id handoff out of scope
4. `m246-launcher-profile-picker-selection`
   - done. Implemented selected-id handoff that re-reads the store, validates
     launchability, starts the existing gateway/session-config flow only after
     a valid token-protected selection, and returns redacted session config
5. `m247-profile-picker-product-smoke`
   - done. Added Playwright product smoke for default store list, disabled
     resolver-required profiles, selected-id launch, one-use bootstrap and
     selection, fake OpenSSH argv, and redaction checks
6. `m248-host-owned-import-ui-plan`
   - planned. Plan OpenSSH import preview/confirm UI only after profile picker
     redaction and token patterns are proven

## Test Plan

For the first implementation slice, verify:

- summary helper includes id, name, tags, default marker, and launchability
- summary helper counts launchable vs resolver-required profiles
- summary serialization excludes host, user, port, jump host, identity path,
  config path, extra args, remote command, credential secret id, and JSON store
  internals
- wasm target can compile the pure summary types
- existing profile-store CLI and launcher tests still pass

For launcher/browser slices, verify:

- profile picker route is loopback-only
- route requires the launcher UI token
- route returns redacted summary only
- selected id is validated by re-reading the store
- stale or wrong tokens fail
- browser smoke does not expose profile internals in DOM text, session config,
  console diagnostics, or network payloads except the selected id

## Non-Goals

- in-window live transport replacement
- browser-side local file reading
- browser-side profile editing
- plugin-owned profile store reads or writes
- vault resolver
- SFTP/tunnel/bookmark UI
- team sync
- OpenSSH import UI before the profile picker route is proven
