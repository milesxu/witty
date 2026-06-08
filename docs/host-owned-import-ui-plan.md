# Host-Owned OpenSSH Import UI Plan

Updated: 2026-06-01

## Purpose

`m248-host-owned-import-ui-plan` defines the next profile-store UI boundary
after the launcher-owned SSH profile picker proved the short-lived token,
redacted bootstrap, selected-id handoff, and fake-OpenSSH smoke pattern.

The goal is to make OpenSSH config import reviewable from a trusted Witty
UI without giving browser JavaScript, wasm plugins, terminal output, or plugin
commands access to raw OpenSSH config contents, raw `SshProfile` candidates,
local paths, OpenSSH argv, or credential references.

This is a plan only. It does not add an import UI route or any browser write
surface.

## Current Foundation

Already implemented:

- native OpenSSH import preview CLI:
  `witty --profile-store-import-openssh-preview <path> [--profile-store <path>]`
- native confirmed import write CLI:
  `witty --profile-store-import-openssh <path> --confirm ...`
- pure parser and apply helpers in `witty-transport`
- locked `edit_profile_store(..., CreateIfMissing, ...)` confirmed write
  transaction
- redacted profile-store summaries
- launcher-owned profile picker with one-use bootstrap and token-protected
  selected-id handoff
- browser product smoke with fake `ssh` and redaction checks

The import UI should reuse these primitives rather than inventing a separate
browser-owned import pipeline.

## Boundary Decision

The first import UI must be host-owned and native-file-backed.

Allowed first shape:

```text
witty --web --profile-import-openssh <config-path> [--profile-store <path>]
```

Native code reads the explicit config path, parses a fresh preview, resolves
the target profile store for conflict detection, and serves only a redacted
review model to the browser shell. Browser code can select candidate ids and
submit confirmation, but native code re-parses the config and re-runs the
confirmed write transaction before mutating the store.

Deferred shapes:

- native file picker for choosing a config file
- opening import review from inside the profile picker
- browser drag/drop or file input
- plugin-requested import UI

The initial CLI-supplied path is less ergonomic, but it preserves a clear trust
boundary while the UI and transaction semantics are proven.

## Redacted Review Model

Add a pure redacted preview helper before adding routes:

```rust
pub struct OpenSshImportReview {
    pub candidates: Vec<OpenSshImportCandidateSummary>,
    pub selected_by_default: Vec<String>,
    pub warning_count: usize,
    pub global_warning_count: usize,
    pub conflict_count: usize,
}

pub struct OpenSshImportCandidateSummary {
    pub id: String,
    pub name: String,
    pub tags: Vec<String>,
    pub warning_count: usize,
    pub has_conflict: bool,
}
```

The review model may include candidate id, candidate name, tags, warning
counts, and conflict status. It must not include target host, user, port, jump
host, identity-file path, config-file path, proxy command, remote command,
OpenSSH extra args, credential secret id, source config path, source line text,
serialized `SshProfile`, or serialized `OpenSshImportPreview`.

Candidate ids and names are still inventory-like data. They are acceptable for
the trusted host-owned browser shell, matching the profile picker boundary, but
they must not be sent to plugins by default.

## Launcher Routes

Use a separate import-review session, not the gateway session or picker token.

Page URL:

```text
http://127.0.0.1:<ui-port>/index.html#profile_import=<review-id>
```

Bootstrap:

```text
GET /profile-import/<review-id>.json
```

Response:

```json
{
  "kind": "profile_import",
  "protocol": 1,
  "ui_token": "<random-ui-token>",
  "confirm_url": "/profile-import/<review-id>/confirm",
  "expires_at_ms": 180000,
  "review": {
    "candidates": [],
    "selected_by_default": [],
    "warning_count": 0,
    "global_warning_count": 0,
    "conflict_count": 0
  }
}
```

Confirmation:

```text
POST /profile-import/<review-id>/confirm
Content-Type: application/json

{
  "ui_token": "<random-ui-token>",
  "profile_ids": ["prod"],
  "conflict": "reject"
}
```

On success, return aggregate-only mutation output:

```json
{
  "changed": true,
  "profiles": 5,
  "default_changed": false,
  "bytes": 1234,
  "created_parent_dir": false,
  "selected": 3,
  "added": 2,
  "replaced": 1,
  "warning_count": 4,
  "global_warning_count": 1
}
```

The success response must not echo selected profile details.

## Confirmation Semantics

Native confirmation must:

1. Verify review id, UI token, TTL, request shape, selected candidate ids, and
   conflict policy.
2. Re-read the explicit OpenSSH config path.
3. Re-parse the preview.
4. Re-read or create the target profile store through the existing edit path.
5. Apply selected ids through `apply_openssh_import_preview(...)`.
6. Use the same `reject` and exact-id `replace` semantics as the CLI.
7. Return aggregate-only counts.

The browser review model is not trusted. It is display state only.

Selection policy:

- default UI selection can include non-conflicting candidates
- conflicting candidates require explicit user selection plus `replace`
- duplicate or unknown ids fail the whole confirmation
- empty confirmation fails
- warning counts do not block confirmation, but they remain visible in summary

## Browser State

Add a sibling pre-session state machine:

```text
boot
  -> profile_import_loading
  -> profile_import_ready
  -> profile_import_confirming
  -> profile_import_done
  -> profile_import_error
```

The UI should use normal HTML controls:

- checkbox per redacted candidate summary
- conflict policy segmented control: `reject` and `replace`
- disabled confirm button when no candidate is selected
- aggregate warning/conflict count display

The first import UI should not auto-open a terminal gateway after success. It
can show completion and a link/action to return to profile picker in a later
slice.

## Security Rules

- Config path stays in native code and does not appear in URL, DOM text, JSON,
  console diagnostics, or smoke output.
- Raw config contents never go to the browser.
- Raw `OpenSshImportPreview`, `OpenSshImportCandidate`, and `SshProfile`
  objects never go to the browser.
- The UI token is separate from gateway tokens and profile-picker tokens.
- Bootstrap is one-use and no-store.
- Confirmation is one-use and TTL-bound.
- Confirmation re-parses and re-applies from native inputs; browser-selected
  ids are lookup keys only.
- Writes use the existing locked edit transaction.
- Plugins do not receive summaries, candidate ids, raw previews, or mutation
  reports until a dedicated permission model exists.
- Logs use route names, status codes, and aggregate counts only.

## Implementation Split

1. `m249-import-review-redacted-types`
   - done. Added pure redacted review summary types/helpers in
     `witty-transport`
   - asserted serialization omits host, user, paths, argv, commands, source
     paths, and raw profile/import internals
2. `m250-launcher-import-review-api`
   - done. Added explicit `--profile-import-openssh <path>` parsing under
     `--web`
   - added import-review session, one-use bootstrap, and redacted route
   - kept confirmation/write out of scope
3. `m251-launcher-import-confirm-api`
   - done. Added token-protected confirmation route
   - re-reads config, re-parses preview, and runs existing confirmed write
     helper under the locked profile-store edit transaction
   - returns aggregate-only JSON
4. `m252-profile-import-product-smoke`
   - done. Added `WITTY_WEB_SMOKE_GATEWAY=profile-import` Playwright smoke
     with a temporary OpenSSH config and profile store
   - verifies redacted DOM/URL/report exposure, stale bootstrap behavior,
     replace confirmation, exact aggregate mutation counts, exact store
     contents, and clean launcher exit after confirmation
   - confirmation one-use and reject-conflict store preservation remain covered
     by focused launcher tests
5. `m253-profile-picker-import-entry-plan`
   - done. Decided that picker import entry must use native-preauthorized
     import sources, starting with
     `--profile-picker-import-openssh <config-path>` under `--profile-picker`
   - planned a token-protected picker import action route that returns only a
     random `#profile_import=<review-id>` URL and consumes the picker session
   - see `profile-picker-import-entry-plan.md`
6. `m254-picker-import-source-binding`
   - done. Added the native CLI/source binding and redacted picker bootstrap
     import action without adding browser-owned local path selection
7. `m255-picker-import-action-route`
   - done. Added the token-protected picker import action route that creates an
     import review session from the native-owned binding
8. `m256-picker-import-product-smoke`
   - done. Added product smoke for picker action -> import review -> confirmed
     write
9. `m257-post-import-picker-refresh`
   - done. Successful picker-owned import confirmation returns a redacted
     `next_picker_url` and registers a fresh picker session over the updated
     profile store
   - the refreshed picker keeps native-owned import actions, old picker/import
     tokens stay one-use, and the product smoke launches an imported profile
     from the refreshed picker
10. `m258-refreshed-picker-reimport-boundary`
   - done. Added launcher coverage for entering the native-owned import action
     again from the refreshed picker, proving the review is rebuilt against the
     updated store and imported ids are now redacted conflicts
11. `m259-import-review-segmented-conflict-summary`
   - done. Added aggregate warning/conflict/global count display to the
     host-owned import review, replaced the conflict-policy select with a
     `reject`/`replace` segmented control, and covered the UI state in product
     smoke without exposing raw OpenSSH details
12. `m260-import-review-conflict-selection-guard`
   - done. The review UI now keeps conflicting candidates disabled under
     `reject`, re-enables them only after `replace` is selected, and product
     smoke confirms through the rendered button instead of bypassing the UI
     selection state
13. `m261-import-review-reject-product-smoke`
   - done. Added standalone browser smoke coverage for confirming the default
     `reject` path without selecting conflicts. The smoke proves the existing
     conflicting profile remains unchanged, the non-conflicting profile is
     added, and report counts stay aggregate-only.
14. `m262-import-review-completion-summary`
   - done. Import completion now renders selected/added/replaced/warning
     counts in the host-owned review UI, and product smoke checks the visible
     summary for both reject and replace paths without exposing profile
     internals.
15. `m263-import-review-next-picker-button-smoke`
   - done. Picker-owned import completion now has smoke coverage for the
     visible `Profiles` button state and click path back to the refreshed
     picker.
16. `m264-import-review-standalone-next-picker-negative-smoke`
   - done. Standalone import completion now has smoke coverage that the
     return-to-picker button remains hidden, while picker-owned import is the
     only flow that exposes it.
17. `m265-import-review-accessibility-smoke`
   - done. The import review conflict-policy group and aggregate result summary
     now carry explicit accessibility metadata, and smoke checks those
     attributes before confirmation.
18. `m266-import-review-completed-control-lock-smoke`
   - done. Completion smoke now checks that the review is no longer editable
     after confirmation: candidates, conflict policy, and import button remain
     disabled, while the selected conflict policy remains visible.
19. `m267-import-review-conflict-toggle-smoke`
   - done. The replace smoke now covers switching back to `reject` after a
     conflict was selected, verifying the conflict checkbox is cleared and
     disabled before switching back to `replace`.
20. `m268-import-review-empty-selection-disable-smoke`
   - done. Browser smoke now covers the empty-selection guard in the review UI:
     unchecking the only selectable default candidate disables `Import`, and
     re-checking it restores the button.
21. `m269-import-review-conflict-button-click-smoke`
   - done. Replace smoke now switches conflict policy through actual segmented
     button clicks, covering the same interaction a user performs in the review
     UI.
22. `m270-import-review-candidate-checkbox-click-smoke`
   - done. Candidate selection smoke now uses real checkbox clicks for clearing
     and restoring the default selection and for selecting conflicts under
     `replace`.
23. `m271-import-review-completed-policy-helper-freeze`
   - done. Completed/disabled reviews now ignore conflict-policy helper changes
     so the visible selected policy cannot change after confirmation starts.
24. `m272-import-review-completed-disabled-helper-freeze`
   - done. Completed reviews now stay locked even if the disabled-state helper
     is asked to re-enable controls after confirmation.
25. `m273-import-review-next-picker-url-authorization`
   - done. The post-import return-to-picker helper now accepts only the
     server-provided `next_picker_url` already present in the aggregate import
     report. Smoke verifies forged picker URLs are rejected, standalone imports
     keep the button hidden, and picker-owned imports keep the real button
     intact.
26. `m274-import-review-report-helper-authentication`
   - done. Import review setup now resets stale completion/report helper state,
     and the report helper ignores synthetic objects that were not produced by
     the active confirmation request. Smoke covers a forged report before
     confirmation and verifies the review remains interactive and redacted.
27. `m275-import-review-report-helper-replay-freeze`
   - done. The accepted completion report is now one-shot for UI rendering:
     subsequent helper calls return the frozen aggregate summary and cannot
     recompute result text or next-picker authorization from mutated report
     fields.
28. `m276-import-review-report-weakset-auth`
   - done. The report helper now trusts only report objects registered in an
     internal WeakSet by the confirmation flow, not the writable window report
     pointer. Confirmed reports are frozen before exposure, with smoke coverage
     for global-pointer spoofing and post-completion mutation attempts.
29. `m277-import-review-result-summary-freeze`
   - done. The aggregate completion summary exposed to browser helpers is now
     frozen, preventing later page code from mutating the selected/added/
     replaced/warning counts after completion.
30. `m278-import-review-preview-summary-freeze`
   - done. The redacted preview candidate list and aggregate review summary are
     now frozen as soon as the review renders, so helper consumers cannot
     rewrite displayed import inventory metadata before confirmation.
31. `m279-profile-picker-exposed-summary-freeze`
   - done. The profile picker now freezes helper-exposed redacted profiles,
     profile tags, and native import actions, keeping picker inventory/action
     metadata stable before and after import refresh.
32. `m280-bootstrap-exposure-snapshot-freeze`
   - done. Browser helpers now receive frozen bootstrap snapshots for profile
     picker and import review routes, while internal mutable bootstrap state
     remains private to the flow. Smoke proves exposed URL/token mutation does
     not alter selection, import entry, or confirmation behavior.
33. `m281-picker-import-entry-freeze`
   - done. The picker-owned import entry returned by the native action route is
     frozen before it is exposed, and the delayed navigation captures the
     validated URL string so page code cannot rewrite the next import route.
34. `m282-one-use-helper-token-guard`
   - done. Browser helper calls for picker selection, picker-owned import, and
     import confirmation now short-circuit after token consumption instead of
     sending empty-token requests or moving the visible flow into an error
     state.
35. `m283-one-use-helper-in-flight-guard`
   - done. Browser helper calls for picker selection/import and import
     confirmation now short-circuit while the first one-use request is in
     flight, preventing concurrent helper re-entry from sending a duplicate
     native request or racing the UI into an error state.
36. `m284-retry-clears-stale-helper-errors`
   - done. Recoverable picker/import helper failures no longer leave stale
     `LastError` objects attached to later successful retries. Browser smoke
     covers bad import conflict retries before the real one-use flow completes.
37. `m285-malformed-success-consumes-helper-token`
   - done. Once a picker/import helper receives `200 OK`, the browser now
     clears the one-use UI token before JSON parsing or shape validation.
     Smoke injects malformed successful responses through same-origin fake
     routes and verifies the browser cannot retry with a token that native may
     already have consumed.
38. `m286-next-picker-url-strict-validation`
   - done. The post-import return-to-picker helper now accepts only the exact
     relative refreshed picker URL shape generated by native code:
     `/index.html#profile_picker=<32 lowercase hex>`. Smoke rejects
     prefix-looking but malformed URLs before following the real refreshed
     picker.
39. `m287-picker-import-url-strict-validation`
   - done. The picker-owned import entry helper now accepts only the exact
     relative import review URL shape generated by native code:
     `/index.html#profile_import=<32 lowercase hex>`. Smoke rejects a
     prefix-looking malformed review id and does not follow the forged import
     URL.
40. `m288-bootstrap-action-url-strict-validation`
   - done. Import review bootstrap now accepts confirmation endpoints only when
     they exactly match `/profile-import/<current-review-id>/confirm`, and
     profile import hash ids must be 32 lowercase hex before any bootstrap
     fetch. Smoke keeps malformed confirmation tests on legal ids and verifies
     picker-owned import still completes.
41. `m293-redacted-bootstrap-field-validation`
   - done. Browser bootstrap loading now validates redacted summary field
     shapes and consistency before rendering ready states: profile/candidate
     tags must be display-safe strings, default ids must match boolean
     `is_default` flags, launchability/credential/warning/conflict counts must
     match their lists, and default import selections cannot reference
     conflicts, missing ids, or duplicate ids. Picker import actions are
     allowlisted before sending a token, and import confirmation reports must
     pass aggregate field validation before the review enters done. Browser
     smoke injects malformed `200 OK` bootstrap/report JSON and verifies the UI
     fails closed without exposing helper snapshots as ready.
42. `m294-redacted-bootstrap-field-whitelist`
   - done. Browser import bootstrap/review/report validation now rejects any
     field outside the redacted Rust structs. This explicitly blocks accidental
     `store_path`, `config_path`, host, or other raw native data from becoming
     accepted UI state, and smoke covers extra fields on envelopes and
     candidates while preserving real picker-owned import completion.
43. `m295-redacted-dto-deny-unknown-fields`
   - done. Rust redacted import review and confirmation report DTOs now reject
     unknown JSON fields during deserialization. Unit tests cover top-level
     path-like extras and nested candidate host-like extras so the type layer
     matches the browser field whitelist.
44. `m297-action-response-field-whitelist`
   - done. Picker-owned import entry responses now reject unsupported fields
     before the browser exposes helper state or navigates to the import review.
     Smoke covers a path-like extra field in import-entry JSON while preserving
     the real picker-owned import flow.
45. `m298-bootstrap-envelope-deny-unknown-fields`
   - done. Rust import bootstrap envelopes now reject unknown fields during
   deserialization, matching browser-side envelope whitelisting for host-owned
   import review UI.
46. `m299-confirm-request-local-validation`
   - done. Browser import confirmation now rejects invalid local requests
     before sending the one-use token: unsupported conflict policies, empty
     selections, duplicate ids, unknown ids, and `reject` requests that include
     a conflicting candidate. Smoke verifies these helper calls leave the ready
     state, token, controls, report, and last confirmation promise unchanged
     before the real button confirmation succeeds.
47. `m300-native-action-request-field-whitelist`
   - done. Native action request DTOs now have direct unit coverage for
     rejecting unknown fields on picker selection, picker import-action, and
     import confirmation request bodies. The confirmation route therefore
     keeps accepting only token, selected ids, and conflict policy, not
     store/config/path-like caller-supplied metadata.
48. `m301-native-action-json-content-type`
   - done. Native import confirmation POST routes now require
     `application/json` before parsing token-protected bodies. Missing,
     form-encoded, or duplicate `Content-Type` inputs fail before confirmation
     validation or store mutation, and browser smoke verifies text/plain
     confirmation probes return 415 without consuming the one-use token.
49. `m302-native-picker-action-claim-before-side-effects`
   - done. Picker-owned import now claims the one-use picker action before
     building a native review session, so concurrent action POSTs cannot create
     duplicate review side effects. Recoverable build/serialization failures
     release the claim for retry.
50. `m303-native-confirm-request-preflight`
   - done. Native confirmation preflights selected ids against the review
     before claiming the one-use state. Duplicate ids, unknown ids, duplicate
     candidate ids, empty selections, and `reject` selections containing a
     conflict fail without consuming the confirmation token. Browser smoke
     covers both helper-local rejection and a direct JSON POST to the native
     route.
51. `m304-native-confirm-apply-retry`
   - done. Native confirmation releases the one-use state when the
     transactional apply/write path fails before a confirmed write report is
     produced, so transient config/store failures can be retried. Failures
     after the store may have been written stay consumed. Unit tests cover
     duplicate candidate ids in the review and apply-failure retry after the
     OpenSSH config is restored. Browser smoke forces a helper-visible 409 and
     then completes the restored confirmation with the same token.
52. `m305-picker-helper-local-validation`
   - done. Browser picker helpers now perform local allowlist checks for
     launchable profile ids and native import action ids before creating
     last-promise state or sending tokens. Smoke verifies invalid selection and
     import helper calls leave ready state, token, controls, and diagnostics
     unchanged before the valid flow continues.
53. `m306-native-protected-route-not-found`
   - done. Native launcher HTTP now treats malformed or unknown protected
     session/picker/import route ids as no-store 404, rather than letting them
     fall through to generic POST method handling or static assets. Unit tests
     verify forged picker/import/session routes return 404.
54. `m307-native-protected-prefix-not-found`
   - done. The no-store 404 fallback now applies to every path under the
     protected session, picker, and import prefixes. Unit tests include extra
     path segments after protected action/bootstrap routes to keep probes out
     of generic method and static-file handling.
55. `m308-native-malformed-http-bad-request`
   - done. Native HTTP parse failures now return no-store 400, covering
     duplicate `Content-Type`, duplicate/invalid `Content-Length`, oversized
     declared body length, and malformed request-line probes in unit tests.
56. `m309-native-request-line-strictness`
   - done. Native HTTP request-line parsing now requires exactly method, path,
     and `HTTP/1.0`/`HTTP/1.1` version tokens with an origin-form path. Unit
     coverage checks unsupported versions, extra tokens, and absolute-form URLs
     produce no-store 400.
57. `m310-native-header-line-strictness`
   - done. Native HTTP now rejects malformed header lines before protected
     import routing sees the request. Unit coverage checks no-colon headers,
     folded lines, empty names, and invalid field-name characters produce
     no-store 400.
58. `m311-native-request-line-sp-strictness`
   - done. Native HTTP request lines now require single-space separators and a
     token-shaped method. Unit coverage checks tab-separated, double-space, and
     invalid-method probes produce no-store 400.
59. `m312-native-header-terminator-required`
   - done. Native HTTP now rejects partial requests that close before
     `\r\n\r\n`. Unit coverage checks missing-terminator and empty request
     probes produce no-store 400 before import routing.
60. `m313-native-header-limit-after-terminator`
   - done. Native HTTP now enforces the 16 KiB header limit even when a
     complete terminator is present in the oversized buffer. Unit coverage
     checks the oversized terminated header returns no-store 400.
61. `m314-web-asset-manifest-field-whitelist`
   - done. Web asset manifest parsing now rejects unknown top-level and asset
     fields, keeping generated static metadata explicit before import UI files
     are served.

## Non-Goals

- browser-side local file reading
- browser text input for arbitrary local file paths
- plugin-owned import preview or writes
- recursive OpenSSH `Include` expansion
- `ProxyCommand` import
- token, environment, or `~` expansion
- automatic conflict renaming
- default-profile selection during first UI import write
- native file picker
- team sync or cloud sync
