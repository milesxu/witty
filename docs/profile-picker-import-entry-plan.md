# Profile Picker Import Entry Plan

Updated: 2026-06-01

## Purpose

`m253-profile-picker-import-entry-plan` decides how the launcher-owned profile
picker should expose the already-proven OpenSSH import review flow.

The picker entry must preserve the host-owned boundary:

- browser code must not provide arbitrary local file paths
- browser code must not receive raw OpenSSH config contents
- browser code must not receive raw `SshProfile` candidates or profile-store
  internals
- plugins and terminal output must not receive import summaries, selected ids,
  paths, or mutation reports by default

The standalone route from `m248` through `m252` remains valid:

```text
witty --web --profile-import-openssh <config-path> [--profile-store <path>]
```

The picker entry is an ergonomic bridge on top of that route, not a new
browser-owned import pipeline.

## Current Baseline

Already implemented:

- launcher-owned profile picker:
  `witty --web --profile-picker [--profile-store <path>]`
- one-use redacted picker bootstrap and token-protected selected-id handoff
- standalone host-owned import review:
  `witty --web --profile-import-openssh <path> [--profile-store <path>]`
- one-use redacted import bootstrap and token-protected confirmation
- confirmation re-reads OpenSSH config, re-parses preview, and writes through
  the locked profile-store edit transaction
- product smoke for standalone import review/confirm

Missing:

- a safe profile-picker affordance that starts import review without exposing a
  local path text field or browser file reader
- a server lifecycle that can own both picker and import-review sessions
- a post-import refresh decision

## Boundary Decision

Initial picker import entry must use a native-preauthorized import source.

Recommended first command shape:

```text
witty --web \
  --profile-picker \
  [--profile-store <path>] \
  --profile-picker-import-openssh <config-path>
```

Rules:

- `--profile-picker-import-openssh <config-path>` is only valid with
  `--profile-picker`.
- The config path is chosen by native CLI invocation, not by browser input.
- The path never appears in page URL, picker bootstrap JSON, import bootstrap
  JSON, DOM text, console diagnostics, or smoke summaries.
- The existing standalone `--profile-import-openssh <config-path>` keeps its
  current mutually exclusive import-review mode.
- A future native file picker can produce the same preauthorized source binding
  without changing browser route semantics.

Do not add a browser text box for local paths, browser drag/drop import, or
plugin-requested import entry in this slice.

## Picker Bootstrap Extension

Extend the picker bootstrap with optional redacted import actions:

```json
{
  "kind": "profile_picker",
  "protocol": 1,
  "ui_token": "<random-ui-token>",
  "selection_url": "/profile-picker/<picker-id>/select",
  "import_url": "/profile-picker/<picker-id>/import",
  "expires_at_ms": 180000,
  "summary": { "profiles": [] },
  "import_actions": [
    {
      "id": "openssh-config",
      "kind": "openssh_config",
      "label": "OpenSSH Import"
    }
  ]
}
```

The first action model intentionally avoids candidate counts and warning counts
so the picker does not need to parse the import source before the user asks for
review. Those counts are already available in the redacted import review after
the action starts.

The action model must not include:

- config path
- profile-store path
- host/user/port/jump-host values
- identity-file path
- remote command or OpenSSH argv
- source line metadata
- raw preview or raw profile JSON

## Import Action Route

Add a token-protected route owned by the profile picker session:

```text
POST /profile-picker/<picker-id>/import
Content-Type: application/json

{
  "ui_token": "<random-ui-token>",
  "action_id": "openssh-config"
}
```

On success, native code creates a new import-review session from the
preauthorized config path and the picker's profile-store path, then returns only
a random review URL:

```json
{
  "kind": "profile_import_entry",
  "protocol": 1,
  "import_url": "/index.html#profile_import=<review-id>"
}
```

Browser behavior:

```text
picker ready
  -> user activates OpenSSH Import
  -> POST picker UI token + action id
  -> navigate to returned import_url
  -> existing #profile_import loader fetches redacted review
  -> existing confirmation UI writes through native confirmation route
```

Native behavior:

```text
verify picker id, token, TTL, and unused state
  -> resolve action id to preauthorized config path
  -> read config and current target profile store
  -> build ProfileImportReviewSession
  -> mark picker session as ImportReviewStarted
  -> return only #profile_import=<review-id>
```

The browser-returned action id is a lookup key only. It is never a path.

## Session Lifecycle

For the first implementation, keep lifecycle conservative:

```text
PickerPending
  -> SelectedProfile
  -> ImportReviewStarted
  -> Expired
```

Only one terminal launch or import review can be started from a picker session.
After import review starts, stale picker selection/import requests fail with a
terminal HTTP status.

Standalone import review keeps the current behavior: successful confirmation
can end the launcher process.

Picker-owned import review should be prepared to support a later return to a
fresh picker session, but the first slice may show the existing import-done
state after confirmation. A later product slice can add a native-owned refresh
route or a new picker URL in the aggregate success response.

## Post-Import Refresh Decision

Do not reload the original one-use picker bootstrap after import confirmation.
That would either violate the one-use inventory boundary or require keeping the
old picker token valid.

Preferred later shape:

```json
{
  "changed": true,
  "profiles": 5,
  "default_changed": false,
  "selected": 3,
  "added": 2,
  "replaced": 1,
  "next_picker_url": "/index.html#profile_picker=<new-picker-id>"
}
```

`next_picker_url` is a random URL created by native code after a successful
picker-owned import. It contains no store path or profile details. The new
picker bootstrap returns a freshly read redacted summary from the updated store.

This refresh can be implemented after the entry route itself is proven.

## Security Rules

- Browser requests never contain local filesystem paths.
- Import action ids are native-issued opaque identifiers.
- Picker import action consumes the picker session for this first design.
- Import review still re-reads the OpenSSH config and profile store at
  confirmation time; the browser review remains display state only.
- Import confirmation keeps aggregate-only success output.
- Plugins receive neither picker summaries nor import actions.
- Logs should mention route names, status codes, and aggregate counts only.

## Suggested Task Split

1. `m254-picker-import-source-binding`
   - done. Added `--profile-picker-import-openssh <path>` parsing and
     validation under `--profile-picker`
   - extended launcher config/session state with native-owned import source
     bindings
   - extended picker bootstrap with redacted `import_url` and `import_actions`
   - added serialization tests proving config/store paths and raw profile data
     stay out of the picker bootstrap
2. `m255-picker-import-action-route`
   - done. Added token-protected `POST /profile-picker/<id>/import`
   - creates import-review sessions from native-owned bindings and returns only
     a random `#profile_import=<review-id>` URL
   - consumes the picker session after import review starts
   - reuses existing `#profile_import` browser loader and confirmation route
3. `m256-picker-import-product-smoke`
   - done. Added `WITTY_WEB_SMOKE_GATEWAY=profile-picker-import`
     Playwright coverage that starts in profile picker, enters import review,
     confirms replace import, verifies redaction and exact store mutation, and
     checks stale picker/import routes
4. `m257-post-import-picker-refresh`
   - done. Picker-owned import confirmation now returns an aggregate-only
     report plus `next_picker_url`
   - the original one-use picker/import tokens remain stale, while the native
     launcher registers a fresh redacted picker bootstrap from the updated
     profile store
   - product smoke now follows picker action -> import review -> confirm ->
     refreshed picker -> imported profile launch
5. `m258-refreshed-picker-reimport-boundary`
   - done. Added focused launcher coverage that the refreshed picker keeps its
     native-owned import action, can start a new import review from the updated
     store, marks newly imported ids as conflicts, and remains one-use after
     that second action
   - verified the second import entry and refreshed review still expose only
     random route ids plus redacted candidate summaries
6. `m259-import-review-segmented-conflict-summary`
   - done. Replaced the import review conflict-policy select with a compact
     segmented control, exposed aggregate warning/conflict/global counts in the
     review panel, and extended product smoke coverage for default `reject`,
     switching to `replace`, and exact redacted summary counts
7. `m260-import-review-conflict-selection-guard`
   - done. Conflict candidates now stay unchecked and disabled while the review
     is in `reject` mode, become selectable after switching to `replace`, and
     product smoke confirms import through the real review button path
8. `m261-import-review-reject-product-smoke`
   - done. Added a standalone `profile-import-reject` product smoke mode for
     the default conflict policy path: it imports only the non-conflicting
     `staging` candidate, keeps the existing `prod` profile unchanged, and
     verifies aggregate-only report counts through the rendered review button
9. `m262-import-review-completion-summary`
   - done. Successful import confirmation now renders an aggregate-only result
     summary in the review panel, covering selected/added/replaced/warning
     counts while keeping raw profile and OpenSSH details out of DOM text
10. `m263-import-review-next-picker-button-smoke`
   - done. Picker-owned import smoke now verifies the rendered `Profiles`
     button after confirmation and clicks that real UI control to enter the
     refreshed picker, instead of using the window helper directly
11. `m264-import-review-standalone-next-picker-negative-smoke`
   - done. Standalone import smokes now assert the post-confirm `Profiles`
     button remains hidden while picker-owned import keeps the visible button,
     preserving the route boundary between standalone and picker-owned flows
12. `m265-import-review-accessibility-smoke`
   - done. Added explicit accessible labels/live-region metadata for the
     conflict-policy control and aggregate result summary, with product smoke
     assertions so future DOM changes preserve those semantics
13. `m266-import-review-completed-control-lock-smoke`
   - done. Product smoke now verifies that completed import reviews keep
     candidate checkboxes, conflict-policy buttons, and the import button
     disabled, while preserving the selected conflict-policy state
14. `m267-import-review-conflict-toggle-smoke`
   - done. Replace-flow smoke now exercises a user toggling back to `reject`
     after selecting a conflicting candidate, proving the conflict selection is
     cleared and disabled until `replace` is selected again
15. `m268-import-review-empty-selection-disable-smoke`
   - done. Import review smoke now verifies that clearing the only currently
     selectable candidate disables the `Import` button, and re-selecting it
     restores the button before confirmation continues
16. `m269-import-review-conflict-button-click-smoke`
   - done. Replace-flow smoke now changes conflict policy by clicking the
     rendered `reject`/`replace` segmented buttons, rather than calling the
     helper directly, so policy switching follows the visible UI path
17. `m270-import-review-candidate-checkbox-click-smoke`
   - done. Import review smoke now changes candidate selection through real
     checkbox clicks for empty-selection and replace-conflict flows, instead of
     mutating checked state directly
18. `m271-import-review-completed-policy-helper-freeze`
   - done. Once the review is disabled for confirmation/completion, the
     conflict-policy helper no longer changes the selected policy or visible
     segmented state, and smoke verifies the completed state is frozen
19. `m272-import-review-completed-disabled-helper-freeze`
   - done. Successful import completion now locks the review so later
     `wittySetProfileImportDisabled(false)` calls cannot re-enable
     candidates, conflict policy, or the import button; smoke verifies the
     disabled helper remains frozen after completion
20. `m273-import-review-next-picker-url-authorization`
   - done. The rendered `Profiles` action is now authorized only by the
     aggregate `next_picker_url` returned in the native import report. Product
     smoke proves standalone imports cannot reveal it through the helper, and
     picker-owned imports reject a forged picker URL without altering the real
     return button.
21. `m274-import-review-report-helper-authentication`
   - done. Import review rendering now clears stale report/next-picker helper
     state, and the report helper only accepts the exact report object written
     by the confirmation flow. Product smoke verifies a spoofed report cannot
     lock the review, render a fake result, or pre-authorize a forged picker
     URL before confirmation.
22. `m275-import-review-report-helper-replay-freeze`
   - done. Accepted import reports are now rendered once: later helper replays
     return the original aggregate summary without re-reading mutable report
     fields. Smoke mutates the exposed completed report and verifies the result
     summary, authorized picker URL, and visible return button remain fixed.
23. `m276-import-review-report-weakset-auth`
   - done. Report helper authorization moved from the writable
     `window.wittyProfileImportReport` pointer into an internal WeakSet,
     and confirmed reports are frozen before exposure. Smoke covers the
     forged-global-pointer bypass before confirmation and frozen report replay
     after completion.
24. `m277-import-review-result-summary-freeze`
   - done. The exposed aggregate result summary is now frozen before it is
     stored on `window.wittyProfileImportResultSummary`. Smoke mutates the
     completed summary object and verifies the aggregate counts and visible
     result text remain fixed.
25. `m278-import-review-preview-summary-freeze`
   - done. The redacted import preview candidates and aggregate review summary
     exposed to browser helpers are now frozen, including nested tag arrays.
     Smoke attempts to mutate the candidate list, candidate fields, tags, and
     review counts before confirmation and verifies they remain unchanged.
26. `m279-profile-picker-exposed-summary-freeze`
   - done. Profile picker helper exposure now freezes redacted profile
     summaries, nested tag arrays, and import actions. Smoke covers the
     standalone picker, picker-owned import entry picker, and refreshed
     post-import picker so helper consumers cannot rewrite visible inventory or
     action metadata.
27. `m280-bootstrap-exposure-snapshot-freeze`
   - done. Profile picker and import review bootstrap helpers now expose
     frozen URL/token snapshots instead of the internal mutable bootstrap
     objects. Smoke mutates exposed selection/import/confirm URLs and tokens,
     then verifies the real internal flow still uses the original one-use
     session data and clears the exposed token after use.
28. `m281-picker-import-entry-freeze`
   - done. The picker import action entry is now frozen before exposure and the
     navigation timer captures the validated import URL string. Smoke mutates
     the exposed entry before the scheduled reload and verifies the browser
     still opens the native-created import review route.
29. `m282-one-use-helper-token-guard`
   - done. Picker selection, picker import action, and import confirmation
     helpers now ignore repeat calls once their internal one-use token has been
     cleared. Smoke verifies duplicate helper calls return `false`/`null`
     without changing terminal/import-ready/done state or producing error
     state after the successful first use.
30. `m283-one-use-helper-in-flight-guard`
   - done. Picker selection/import and import confirmation helpers now reject
     concurrent duplicate calls while the first one-use request is still
     pending. Smoke races duplicate helper calls against the real launch,
     import-entry, and review-button confirmation paths and verifies only the
     first request drives visible state.
31. `m284-retry-clears-stale-helper-errors`
   - done. Profile picker and import review helpers now clear stale error
     objects when a view renders and when a new request starts. Smoke first
     triggers recoverable bad-action and bad-conflict failures, then retries
     the real import/confirmation flow and verifies the old error is gone.
32. `m285-malformed-success-consumes-helper-token`
   - done. Browser helpers now clear exposed one-use tokens immediately after
     a `200 OK` response, before parsing or validating the JSON body. Smoke
     uses same-origin fake bootstrap routes to return malformed successful
     picker selection, picker import-entry, and import confirmation responses,
     then verifies controls remain locked and the stale token is no longer
     exposed.
33. `m286-next-picker-url-strict-validation`
   - done. Return-to-picker URLs are now accepted only as relative
     `/index.html#profile_picker=<32 lowercase hex>` values with no additional
     hash parameters. Smoke rejects empty ids, non-hex ids, extra hash
     parameters, and absolute URLs before preserving the real native refreshed
     picker URL.
34. `m287-picker-import-url-strict-validation`
   - done. Picker import-entry URLs are now accepted only as relative
     `/index.html#profile_import=<32 lowercase hex>` values with no additional
     hash parameters. Smoke rejects a prefix-looking malformed review id and
     verifies the page remains on the picker error state instead of following
     the forged import URL.
35. `m288-bootstrap-action-url-strict-validation`
   - done. Picker bootstrap now accepts selection/import endpoints only when
     they exactly match `/profile-picker/<current-picker-id>/select|import`,
     and launcher hash ids must be 32 lowercase hex before any bootstrap fetch.
     Smoke keeps malformed-success route tests on legal ids and verifies real
     picker and picker-owned import flows still complete.
36. `m289-launcher-gateway-url-strict-validation`
   - done. Picker selection now normalizes returned session configs only after
     verifying the gateway URL is a tokenless loopback
     `ws://.../witty` endpoint. Smoke injects an external gateway URL
     through a malformed successful selection response and verifies the picker
     consumes the token without opening the gateway.
37. `m290-launcher-hash-exclusive-routing`
   - done. Browser launcher routing now treats `#session`, `#profile_picker`,
     and `#profile_import` as exclusive page routes. Smoke opens a mixed
     `#profile_picker=...&profile_import=...` URL and verifies the app fails
     before loading either bootstrap.
38. `m291-launcher-token-shape-validation`
   - done. Browser launcher bootstraps now require native-shaped 64 lowercase
     hex one-use tokens before using picker, import-review, or gateway session
     state. Smoke mocks use legal fake token shapes, and picker-owned import
     still completes through the native one-use paths.
39. `m292-native-protected-route-id-validation`
   - done. Native protected route parsing now rejects picker/import route ids
     unless they are 32 lowercase hex characters, matching generated session
     ids. Unit tests cover bootstrap, selection, import-entry, and confirmation
     routes; picker-owned import smoke verifies real routes still complete.
40. `m293-redacted-bootstrap-field-validation`
   - done. Browser profile picker/import loading now validates redacted
     summaries, action metadata, import review candidates, aggregate counts,
     default flags, and confirmation reports before entering ready or done
     states. Picker import actions are also allowlisted locally before the
     one-use token is sent.
41. `m294-redacted-bootstrap-field-whitelist`
   - done. Browser profile picker/import bootstrap validation now rejects
     unsupported fields on envelopes, profile summaries, import actions,
     review candidates, and confirmation reports. Smoke injects path/host-like
     extras and verifies the page fails closed instead of accepting accidental
     raw native data.
42. `m295-redacted-dto-deny-unknown-fields`
   - done. Rust redacted profile summary, import review, candidate summary,
     and import confirmation report structs now reject unknown JSON fields
     during deserialization. Unit tests cover top-level and nested extras so
     native DTOs and browser bootstrap validation share the same strict field
     contract.
43. `m297-action-response-field-whitelist`
   - done. Picker selection and picker-owned import action responses now reject
     unsupported JSON fields before connecting, exposing helper state, or
     navigating to an import review. Smoke covers extra `ssh_profile` data in a
     returned session config and extra `config_path` data in an import-entry
     response, and Rust import-entry DTO parsing rejects unknown fields.
44. `m298-bootstrap-envelope-deny-unknown-fields`
   - done. Rust picker/import bootstrap envelope DTOs and import action
     metadata now reject unknown fields. Tests ensure empty `import_actions`
     remains optional on serialized picker bootstraps while path-like extras
     are rejected at envelope and action levels.
45. `m299-confirm-request-local-validation`
   - done. Import confirmation helpers now validate conflict policy and
     selected ids locally before mutating helper promise state or sending the
     one-use token. Smoke covers invalid policy, empty selection, duplicate
     ids, unknown ids, and reject-conflict selection before the picker-owned
     import confirmation still completes normally.
46. `m300-native-action-request-field-whitelist`
   - done. Native picker selection/import-action and import confirmation
     request DTOs reject unknown JSON fields, with unit tests for
     store/config/path-like extras. This protects the picker-owned import flow
     even if request bodies are scripted outside the browser helper contract.
47. `m301-native-action-json-content-type`
   - done. Native picker selection, picker import-action, and import
     confirmation POST routes now require `application/json` content type
     before request parsing. Unit tests cover allowed parameterized JSON and
     rejected missing/form/duplicate headers; browser smoke probes text/plain
     selection/import/confirm requests and then completes the normal one-use
     flows.
48. `m302-native-picker-action-claim-before-side-effects`
   - done. Native picker selection/import actions now reserve the one-use
     picker state before launching the gateway or creating import review state.
     Duplicate POSTs therefore cannot race past validation into duplicate
     side effects, while recoverable build failures release the reservation for
     a later retry.
49. `m303-native-confirm-request-preflight`
   - done. Native import confirmation now validates the selected ids against
     the review snapshot before claiming one-use confirmation state. Duplicate,
     unknown, empty, duplicate-candidate, and reject-conflict requests remain
     retryable bad requests rather than consuming the review. Smoke bypasses
     the browser helper with a direct JSON reject-conflict POST and then proves
     the real confirmation still succeeds.
50. `m304-native-confirm-apply-retry`
   - done. Native confirmation releases the one-use claim when the transactional
     apply/write path fails before a confirmed write report exists, allowing a
     later retry after transient config/store failures. Continuation and
     serialization failures remain consumed because the store may already be
     written. Unit tests cover duplicate-candidate preflight and apply failure
     retry after the OpenSSH config is restored; browser smoke verifies a
     helper-visible 409 preserves the token and still permits the final import.
51. `m305-picker-helper-local-validation`
   - done. Picker browser helpers now reject missing/non-launchable profile ids
     and missing import actions locally before assigning last-promise state or
     sending the one-use token. Smoke covers missing import actions in the
     picker-owned import path and verifies the valid import entry still opens.
52. `m306-native-protected-route-not-found`
   - done. Native protected route fallback now returns no-store 404 for
     malformed or unknown picker/import/session route ids, preventing forged
     action paths from reaching generic method handling. Unit tests cover
     picker import and import confirmation route shapes.
53. `m307-native-protected-prefix-not-found`
   - done. Native protected route fallback now treats all subpaths under
     `/profile-picker/`, `/profile-import/`, and `/session/` as protected
     no-store 404 when not matched earlier. Unit tests cover extra path
     segments on picker import and import confirmation probes.
54. `m308-native-malformed-http-bad-request`
   - done. Request parsing failures now return no-store 400 rather than
     disconnecting without a response. Unit coverage includes duplicate content
     headers and invalid body length metadata on protected picker/import paths.
55. `m309-native-request-line-strictness`
   - done. The native request parser now rejects unsupported HTTP versions,
     extra request-line tokens, and non-origin-form paths with no-store 400
     before picker/import routing sees the request.
56. `m310-native-header-line-strictness`
   - done. The native request parser now fails closed on malformed header
     lines, including missing colons, folded headers, empty field names, and
     invalid field-name characters.
57. `m311-native-request-line-sp-strictness`
   - done. The native request parser now requires single-space request-line
     separators and a valid method token before picker/import route handling.
58. `m312-native-header-terminator-required`
   - done. The native request parser now requires a complete header terminator
     before picker/import route handling, so partial and empty requests return
     no-store 400.
59. `m313-native-header-limit-after-terminator`
   - done. The native request parser now applies the header byte limit even
     after a terminator is found, preventing oversized complete headers from
     reaching picker/import route handling.
60. `m314-web-asset-manifest-field-whitelist`
   - done. Web asset manifest DTOs now deny unknown fields so static asset
     metadata cannot silently carry extra local path/config fields into the
     launcher contract.

## Test Plan

- parser rejects picker import source flags outside `--profile-picker`
- parser rejects standalone `--profile-import-openssh` combined with picker
  launch, direct launch, or raw program launch
- picker bootstrap includes import actions only when native bindings exist
- picker bootstrap serialization omits config/store paths and raw profile data
- bad action id, bad token, stale token, and expired session fail
- import action creates a `#profile_import` URL with no path or token
- stale picker selection/import requests fail after import action starts
- post-import picker refresh returns only a random `#profile_picker=<id>` URL
  and exposes the updated redacted profile summary
- existing standalone import review tests still pass
- browser smoke verifies no sensitive values in DOM, href, report, console
  diagnostics, or route output

## Non-Goals

- browser file picker or drag/drop import
- browser text field for a local path
- plugin-requested import actions
- recursive OpenSSH `Include` expansion
- automatic conflict renaming
- post-import cloud/team sync
- in-terminal or in-plugin profile inventory access
