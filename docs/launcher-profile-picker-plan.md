# Launcher Profile Picker Plan

Updated: 2026-06-01

## Purpose

`m244-launcher-profile-picker-plan` defines the first browser-facing profile
picker for Witty SSH profiles.

The picker must preserve the current trust boundary:

- native launcher code owns local profile-store reads
- native launcher code converts the selected profile into OpenSSH argv
- browser code receives only redacted inventory and the final one-use gateway
  session config
- plugins and terminal output do not receive profile-store data

This plan follows `profile-store-host-owned-command-plan.md` and the existing
one-use launcher session model.

## Current Baseline

Already implemented:

- `ProfileStoreV1::redacted_summary()` in `witty-transport`
- direct trusted launch:
  `witty --web [--profile-store <path>] --ssh-profile-id <id>`
- fallback default store resolution when `--ssh-profile-id` is provided without
  `--profile-store`
- one-use `GET /session/<id>.json` endpoint returning `BrowserSessionConfig`
- gateway token kept out of the page URL
- gateway validates exact UI `Origin` and token
- `witty --web` exits after the single browser/gateway session ends
- browser JS loads session config from `#session=<id>` and then connects to the
  WebSocket gateway

The existing direct-launch path starts the gateway before serving the browser
UI. A profile picker changes that lifecycle: the browser must choose a profile
before a gateway session can be configured.

## Entry Point

Initial picker mode should be explicit:

```text
witty --web --profile-picker [--profile-store <path>]
```

Rules:

- `--profile-picker` enables the pre-session picker.
- `--profile-store <path>` selects the store for the picker.
- If `--profile-store` is omitted, native code resolves the platform default
  profile store path.
- `--profile-picker` is mutually exclusive with:
  - `--ssh-profile-json`
  - `--ssh-profile-id`
  - `--program`
  - `--arg`
- Current direct launch behavior remains unchanged.

This avoids changing the default `witty --web` local-shell behavior. A later
product task can decide whether `witty --web` should auto-open the picker
when a default profile store exists.

## Native Launcher State

Add a launcher mode enum rather than overloading `ssh_profile`:

```text
Direct {
  program,
  args,
  ssh_profile,
}

ProfilePicker {
  store_path,
}
```

Direct mode continues to use the current `LaunchSession`,
`SessionConfigState`, and gateway thread behavior.

Profile-picker mode should create a separate `ProfilePickerSession`:

```text
ProfilePickerSession {
  id,
  ui_token,
  store_path,
  ui_origin,
  ui_url,
  created_at,
  bootstrap_served,
  selection_state,
}
```

`id` is a random 16-byte hex id used in the page fragment. `ui_token` is a
separate random 32-byte hex token returned only by the same-origin bootstrap
endpoint. It is not the gateway token.

Selection state should be single-use:

```text
Pending
Selected { profile_id }
Expired
```

Only the first valid selection can start a gateway session. Concurrent or later
selections return a terminal HTTP error and do not spawn another gateway.

## Routes

The picker can reuse the static asset server. Add new no-store JSON routes.

### Page URL

```text
http://127.0.0.1:<ui-port>/index.html#profile_picker=<picker-id>
```

The page URL contains only a random picker id. It does not contain the UI token,
gateway token, profile id, host, user, or store path.

### Bootstrap

```text
GET /profile-picker/<picker-id>.json
```

Response:

```json
{
  "kind": "profile_picker",
  "protocol": 1,
  "ui_token": "<random-ui-token>",
  "selection_url": "/profile-picker/<picker-id>/select",
  "expires_at_ms": 180000,
  "summary": {
    "profiles": [],
    "default_profile_id": null,
    "launchable_profiles": 0,
    "credential_resolver_required_profiles": 0
  }
}
```

Native code builds `summary` from `read_profile_store(store_path)?` followed by
`store.redacted_summary()?`.

The bootstrap endpoint may be one-use like the current session config endpoint.
If a browser reload should be supported later, add an explicit product decision
instead of making the inventory endpoint freely repeatable.

### Selection

```text
POST /profile-picker/<picker-id>/select
Content-Type: application/json

{
  "ui_token": "<random-ui-token>",
  "profile_id": "prod"
}
```

On success, native code returns the existing `BrowserSessionConfig` JSON shape:

```json
{
  "protocol": 1,
  "gateway_url": "ws://127.0.0.1:<gateway-port>/witty",
  "token": "<gateway-token>",
  "mouse_selection_override": "shift-select",
  "scrollback_lines": 10000,
  "expires_at_ms": 180000
}
```

The response must not include the selected profile target, user, port,
identity-file path, config-file path, OpenSSH argv, remote command, or
credential secret id.

## Selection Flow

Profile-picker launch:

```text
native launcher starts in ProfilePicker mode
  -> resolve explicit/default profile-store path
  -> read and validate store once to fail early
  -> bind UI listener on loopback
  -> bind gateway listener on loopback but do not start gateway yet
  -> generate picker id + UI token
  -> print/open index.html#profile_picker=<picker-id>
browser loads page
  -> fetch /profile-picker/<picker-id>.json
  -> render redacted profile picker
  -> user selects launchable profile id
  -> POST selected id + UI token
native launcher handles selection
  -> verify picker id, UI token, TTL, and unused selection state
  -> re-read profile store from disk
  -> validate selected id exists and is launchable
  -> convert selected profile to OpenSSH-backed LocalPtyConfig
  -> create new gateway token and BrowserSessionConfig
  -> start run_once_on_listener(...) using the already-bound gateway listener
browser receives BrowserSessionConfig
  -> connect to gateway_url?token=<gateway-token>
  -> terminal session proceeds through existing browser gateway path
```

The launch-time store read is for fast feedback and initial summary. The
selection-time store read is authoritative; the browser-returned summary is
never trusted.

## Browser State

Add a browser-side pre-gateway state machine:

```text
boot
  -> direct session config path if #session exists
  -> profile picker path if #profile_picker exists
profile_picker_loading
profile_picker_ready
profile_picker_selecting
profile_picker_connecting
terminal_connected
profile_picker_error
```

The picker UI should display only:

- profile id
- profile name
- tags
- launchability label
- default marker

Profiles with `requires_credential_resolver` should be visible but disabled
until a credential resolver exists. If all profiles require a resolver, show a
non-launching error state rather than hiding the inventory.

The picker should use normal HTML controls before the terminal WebGPU session
starts. Keyboard navigation, Enter, and click selection are sufficient for the
first implementation.

## Failure Handling

Native HTTP responses:

- `400 Bad Request`: malformed JSON, missing token, missing profile id, unsafe
  profile id
- `401 Unauthorized`: wrong UI token
- `404 Not Found`: unknown picker id or unknown selected profile id
- `409 Conflict`: profile exists but currently requires a credential resolver
- `410 Gone`: picker bootstrap already used, selection already consumed, or
  picker expired
- `500 Internal Server Error`: native I/O, parse, validation, bind, or gateway
  startup failure

Browser behavior:

- show a concise error state for bootstrap failures
- keep the picker disabled while selection is in flight
- on selection failure, show the status and allow retry only for non-terminal
  failures
- on successful selection, discard the UI token and connect using the returned
  gateway token

Native logs should use route names, status codes, and counts. They should not
log host, user, local paths, OpenSSH argv, remote commands, secret ids, or raw
store JSON.

## Security Rules

- The profile store path never goes to the browser.
- The redacted summary is the only inventory payload sent to the browser.
- The UI token is separate from the gateway token.
- The UI token is returned only by the same-origin bootstrap response, not in
  the page URL.
- The gateway token is generated only after a valid selection.
- Selection re-reads and validates the store before launching.
- Browser-selected id is treated only as a lookup key.
- The selection endpoint is single-use and TTL-bound.
- The gateway still validates exact UI `Origin` and gateway token.
- Plugins do not receive profile summaries, selections, or session config.

## Implementation Split

### m245-launcher-profile-picker-redacted-api

Status: done. Implemented native launcher `--profile-picker` parsing, explicit
or default profile-store path resolution, a `ProfilePickerSession` with a
separate UI token, one-use TTL-bound bootstrap JSON, and
`GET /profile-picker/<id>.json` serving only `ProfileStoreSummary`.

Write scope:

- `crates/witty-launcher/src/lib.rs`
- `crates/witty-app/src/main.rs`
- focused docs if needed

Tasks:

- add `--profile-picker` parsing and validation
- resolve default profile-store path for picker mode
- add `ProfilePickerSession` / bootstrap JSON types
- add `GET /profile-picker/<id>.json`
- return `ProfileStoreSummary` only
- keep gateway spawn and selection out of scope

Verification:

- route is loopback-only through existing UI listener
- bootstrap route is no-store
- bootstrap route is one-use or explicitly TTL-bound
- payload contains id/name/tags/default/launchability/counts
- payload omits store path, host, user, port, jump host, identity/config paths,
  OpenSSH args, remote command, secret id, and raw JSON internals
- existing direct launcher tests still pass

### m246-launcher-profile-picker-selection

Status: done. Implemented profile selection POST parsing, UI-token validation,
authoritative store re-read, resolver-required rejection, deferred gateway
startup from the pre-bound listener, redacted `BrowserSessionConfig` response,
and browser `#profile_picker` bootstrap/selection/connect handoff.

Write scope:

- `crates/witty-launcher/src/lib.rs`
- `crates/witty-web/static/app.js`
- `crates/witty-web/static/index.html`
- focused tests

Tasks:

- add selection request parsing
- require picker UI token
- re-read store and validate selected profile id
- reject resolver-required profiles
- start the gateway only after valid selection
- return existing `BrowserSessionConfig`
- add browser pre-gateway picker state and selection/connect handoff

Verification:

- wrong UI token fails
- stale picker fails
- duplicate selection fails
- missing id fails
- resolver-required id fails without starting gateway
- success starts one gateway and returns redacted session config only
- `cargo test -p witty-launcher`
- `node --check crates/witty-web/static/app.js`

### m247-profile-picker-product-smoke

Status: done. Added `WITTY_WEB_SMOKE_GATEWAY=profile-picker` coverage to
the existing Playwright smoke harness. The smoke creates a temporary profile
store with launchable/default, resolver-required, and non-default launchable
profiles, puts a fake `ssh` first in `PATH`, selects the launchable profile in
Chromium, verifies one-use picker bootstrap/selection behavior, checks redacted
DOM/URL exposure, confirms the fake OpenSSH argv, and then runs through the
existing browser terminal product smoke path.

Write scope:

- `scripts/run-witty-web-smoke.mjs`
- `scripts/run-witty-web-smoke.sh`
- product smoke docs

Tasks:

- generate a temporary profile store with at least:
  - one launchable profile
  - one disabled resolver-required profile
  - one non-default launchable profile
- run `witty --web --profile-picker --profile-store <temp-store>`
- use a temporary fake `ssh` executable earlier in `PATH` so the selected
  OpenSSH-backed PTY path is deterministic and does not require a real SSH
  server or network
- select a profile in Chromium
- verify the browser reaches the terminal session
- verify page URL, DOM text, network payloads, console logs, and session config
  do not expose host/user/key path/config path/OpenSSH argv/remote command or
  secret id
- verify stale bootstrap/selection behavior where practical

Verification:

```text
WITTY_WEB_SMOKE_GATEWAY=profile-picker scripts/run-witty-web-smoke.sh
```

### m289-launcher-gateway-url-strict-validation

Status: done. Browser session config validation now accepts only tokenless
loopback `ws://.../witty` gateway URLs before adding the one-use token.
The profile-picker smoke injects an external gateway URL through a malformed
successful selection response and verifies the token is consumed without
connecting; direct launcher smoke verifies the native loopback URL still
connects.

Verification:

```text
WITTY_WEB_SMOKE_GATEWAY=profile-picker scripts/run-witty-web-smoke.sh
WITTY_WEB_SMOKE_GATEWAY=launcher scripts/run-witty-web-smoke.sh
```

### m290-launcher-hash-exclusive-routing

Status: done. Browser hash parsing now rejects recognized launcher route keys
when they are mixed with other hash parameters, keeping `#session`,
`#profile_picker`, and `#profile_import` as exclusive page routes. The smoke
harness opens a mixed picker/import hash and verifies the page fails before any
bootstrap fetch, then continues through the real launcher-backed flow.

### m291-launcher-token-shape-validation

Status: done. Browser bootstrap/session validation now accepts only 64
lowercase hex one-use tokens, matching native `random_hex(32)` output. Smoke
mocks use legal fake token shapes so malformed-success tests still exercise the
post-response token-consumption paths.

### m292-native-protected-route-id-validation

Status: done. Native protected route parsing now requires 32 lowercase hex
ids for `/profile-picker/...` and `/profile-import/...` routes before routing
bootstrap, select/import, or confirm requests. Unit tests reject malformed ids,
and picker-owned import smoke verifies generated ids still work.

### m293-redacted-bootstrap-field-validation

Status: done. Browser picker/import bootstraps now validate redacted summary
shape, counts, default flags, tags, action metadata, import defaults, and
confirmation reports before entering ready or done states. Bad picker import
actions are rejected locally before sending the one-use UI token.

### m294-redacted-bootstrap-field-whitelist

Status: done. Browser picker/import validation now rejects unsupported JSON
fields on envelopes, summaries, actions, candidates, and reports. Smoke injects
path/host-like extras and verifies they fail closed.

### m295-redacted-dto-deny-unknown-fields

Status: done. Rust redacted profile/import DTOs now reject unknown JSON fields
during deserialization, with tests for top-level and nested path/host-like
extras.

### m296-session-config-field-whitelist

Status: done. Browser gateway session config loading now rejects unsupported
fields and invalid `expires_at_ms`, and Rust `BrowserSessionConfig` uses
`serde(deny_unknown_fields)`. Smoke covers malformed session JSON before the
normal launcher-backed flows proceed.

### m297-action-response-field-whitelist

Status: done. Picker selection and picker import-entry action responses now
reject unsupported JSON fields before connecting or navigating. Smoke covers an
extra `ssh_profile` object in returned session config and an extra `config_path`
in import-entry JSON, while Rust import-entry parsing also denies unknown
fields.

### m298-bootstrap-envelope-deny-unknown-fields

Status: done. Rust picker/import bootstrap envelopes and import-action metadata
now deny unknown fields during deserialization. Tests parse a real serialized
picker bootstrap with omitted empty actions and reject path-like extras on
envelopes and action metadata.

### m299-confirm-request-local-validation

Status: done. Browser import confirmation helpers now reject unsupported
conflict policies, empty selections, duplicate ids, unknown ids, and
reject-conflict selections locally before assigning a new last-confirm promise
or sending the one-use token. Smoke verifies invalid helper calls preserve the
ready state before the valid native confirmation path still succeeds.

### m300-native-action-request-field-whitelist

Status: done. Native picker selection, picker import-action, and import
confirmation request DTOs reject unknown JSON fields. Unit tests cover
store/config/path-like extras so browser-to-native action payloads stay limited
to the expected token and action fields.

### m301-native-action-json-content-type

Status: done. Native token-protected action POST routes require an
`application/json` media type before parsing request bodies. Missing,
form-encoded, and duplicate content-type headers are rejected, while
parameterized JSON stays accepted for browser fetch compatibility. Smoke also
probes text/plain selection/import/confirm requests before completing the real
one-use helper flows.

### m302-native-picker-action-claim-before-side-effects

Status: done. Native picker selection/import actions now claim one-use picker
state before launching a gateway or constructing import review state, removing
the concurrent duplicate side-effect window. Recoverable failures release the
claim so a later valid retry can still proceed.

### m303-native-confirm-request-preflight

Status: done. Native import confirmation now checks selected ids against the
redacted review before claiming the one-use confirmation state. Empty,
duplicate, unknown, duplicate-candidate, and reject-conflict requests fail as
retryable bad requests instead of consuming the review. Browser smoke includes
a direct JSON reject-conflict POST to the native route before the valid helper
confirmation.

### m304-native-confirm-apply-retry

Status: done. Confirmation apply failures now release the one-use claim because
the native transaction did not produce a confirmed write report. This keeps
transient config/store changes retryable while leaving post-write continuation
and serialization failures consumed. Unit tests cover both duplicate-candidate
preflight and retry after an apply failure caused by the OpenSSH config changing
between review and confirmation. Browser smoke also forces a 409 through the
confirmation helper, then restores the config and completes the import with the
same UI token.

### m305-picker-helper-local-validation

Status: done. Browser picker helpers now check profile ids against the redacted
launchable summary and import action ids against the native allowlist before
creating a last-promise or sending the token. Smoke verifies missing/locked
selection attempts and missing import actions preserve ready state, token,
controls, and last-promise diagnostics before valid flows continue.

### m306-native-protected-route-not-found

Status: done. Native launcher HTTP now gives no-store 404 for protected
session, picker, and import route shapes that are malformed or reference an
unknown valid id, instead of falling through to generic 405/static handling.
Focused unit coverage checks forged protected POST/GET routes while exact active
routes keep their normal method-specific responses.

### m307-native-protected-prefix-not-found

Status: done. The protected-route fallback now covers every path under
`/session/`, `/profile-picker/`, and `/profile-import/`, including extra path
segments after action/bootstrap endpoints. Unit tests verify these probes return
no-store 404 rather than generic method responses or static asset handling.

### m308-native-malformed-http-bad-request

Status: done. Malformed launcher HTTP requests now receive no-store 400 instead
of an implicit disconnect when request parsing fails. Unit tests cover duplicate
content headers, invalid content length, oversized declared bodies, and malformed
request lines on protected routes.

### m309-native-request-line-strictness

Status: done. Native request-line parsing now accepts only exact three-token
`HTTP/1.0` or `HTTP/1.1` origin-form requests. Unit tests verify unsupported
HTTP versions, extra request-line tokens, and absolute-form URLs receive
no-store 400 before any protected route or static asset handling.

### m310-native-header-line-strictness

Status: done. Native header parsing now rejects malformed header lines before
content-type/content-length handling. Unit tests cover missing colons, folded
headers, empty header names, and invalid field names returning no-store 400.

### m311-native-request-line-sp-strictness

Status: done. Native request-line parsing now requires explicit single-space
separators and a token-shaped method, so tab-separated, double-space, and
invalid-method request lines fail closed with no-store 400.

### m312-native-header-terminator-required

Status: done. Native request parsing now requires the `\r\n\r\n` header
terminator before routing. Unit coverage verifies partial headers and empty
requests fail closed with no-store 400 rather than being interpreted as valid.

### m313-native-header-limit-after-terminator

Status: done. Native request parsing now checks the 16 KiB header limit after
finding the terminator too, so an oversized but complete header cannot bypass
the size guard. Unit coverage verifies a terminated oversized header gets
no-store 400.

### m314-web-asset-manifest-field-whitelist

Status: done. Web asset manifest DTOs now deny unknown fields while preserving
the explicit build `generated_by` metadata. Unit coverage rejects unknown
top-level and per-asset fields before the launcher serves static files.

## Non-Goals

- automatic picker launch from plain `witty --web`
- profile editing in the browser
- browser-side OpenSSH import
- native file picker
- vault credential resolver
- SFTP, tunnels, inventory sync, team sync
- plugin access to profile inventory
- in-window native transport replacement
