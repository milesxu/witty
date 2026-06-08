# Plugin Profile Picker Request Action

This slice adds the first host-owned profile flow to the plugin ABI without
giving plugins profile inventory data.

## Contract

The WIT action surface now includes:

```wit
record profile-picker-request {
  reason: option<string>,
}

variant plugin-action {
  request-profile-picker(profile-picker-request),
}
```

The Rust API mirrors this as
`PluginAction::RequestProfilePicker(PluginProfilePickerRequest)`.

## Host Policy

`PluginHost` validates and queues the request; it does not launch a connection
or expose a selected profile back to the plugin in this slice.

Current policy:

- the plugin must have `profile-read`;
- `reason` is optional;
- `reason` must be at most 256 bytes;
- `reason` must not contain control characters.

Accepted requests are stored as `PendingProfilePickerRequest` with the
host-attached `source_plugin`. The plugin does not provide or override the
source plugin id.

Hosts can inspect pending requests with `profile_picker_requests()` and consume
them once with `take_profile_picker_requests()`. The take path clears the queue
so an event loop can hand requests to trusted UI without replaying stale plugin
actions.

## Privacy Boundary

The action does not expose:

- profile ids, names, tags, targets, users, ports, paths, or SSH arguments;
- selected profile id;
- profile store path or default profile id;
- credential resolver or vault references;
- launch success/failure details.

The next step is a host-rendered picker/launch policy that consumes the pending
request and performs selection inside trusted host UI.

For a plugin that already has an opaque profile id, the narrower queue-only
action is `request-profile-launch`; see
`plugin-profile-launch-request-action.md`.

## Host Review Helper

`witty-ui` provides `review_profile_picker_requests()` for trusted host UI. It
takes a current `ProfileStoreV1` plus pending picker requests, revalidates the
store and request reasons, and returns `PendingProfilePickerRequestReview` rows.

Each review row includes:

- source plugin id;
- optional reason;
- redacted `ProfileStoreSummary` for host-rendered picker UI.

The redacted summary may include profile ids, names, tags, launchability, and
default marker because it is host UI data, not plugin ABI data. It still does
not include target host/user/port, OpenSSH paths, extra arguments, remote
command, credential ids, store path, or raw store content.

`TerminalApp::review_pending_profile_picker_requests()` exposes the same
non-mutating review over the app's current queue. It does not drain the queue,
does not select a profile, and does not launch PTY/SSH.

## Host Selection Helper

After trusted host UI selects a profile id, `resolve_profile_picker_selection()`
revalidates the current `ProfileStoreV1`, the original pending picker request,
and the selected id. It returns `ResolvedProfilePickerSelection` only when the
selected profile exists and is launchable.

It returns an error when:

- the selected id is not found;
- the selected profile requires a credential resolver;
- the selected id is unsafe;
- the queued request has an unsafe reason;
- the current profile store fails validation.

On native targets, `resolve_profile_picker_pty_config()` converts the resolved
selection to an SSH `LocalPtyConfig` without spawning a PTY process.

`TerminalApp` exposes non-mutating wrappers for both helpers:

- `resolve_pending_profile_picker_selection()`
- `resolve_pending_profile_picker_pty_config()` on native targets

The helpers leave the pending picker queue intact.

## TerminalApp Convenience

`dismiss_pending_profile_picker_request()` removes one pending picker request by
queue index without resolving a profile id. This is the app-owned cancellation
path for trusted UI. It does not report rejection to the plugin, expose profile
data, or start PTY/SSH.

The broader app-level `review_pending_profile_actions()` helper includes picker
requests as `PendingProfileActionReview::ProfilePicker` rows with
`PendingProfileActionKey::profile_picker(index)`. The key is a current queue
index, so trusted UI should review again after any dismissal or confirmed drain.
`dismiss_pending_profile_action()` can consume the same key for cancellation.
On native targets, `take_resolved_pending_profile_action_pty_config()` can also
consume a `PendingProfileActionConfirmation::ProfilePicker` built from the same
key plus the host-selected profile id.

On native targets, `take_resolved_profile_picker_pty_config()` adds the
confirmed-drain step for one selected pending request. It resolves the selected
profile id against the current `ProfileStoreV1` first; only if resolution
returns a `LocalPtyConfig` does it remove that pending picker request from the
queue. Any missing profile, credential-resolver profile, unsafe selection,
unsafe queued reason, or invalid store leaves the queue intact for trusted UI
error handling or retry.

The helper still does not spawn a PTY or SSH process. It is a deterministic
handoff point for native app policy such as replacing the current session,
opening a new tab, or rejecting the request.

## Verification

- `cargo test -p witty-plugin-api --quiet`
- `cargo test -p witty-plugin-wasm --quiet`
- `cargo test -p witty-ui --quiet`
- `cargo test -p witty-app --quiet`
