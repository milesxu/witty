# Plugin Profile Launch Request Action

This slice adds a host-owned launch request path for plugins that already have
an opaque profile id. It does not let plugins launch connections directly.

## Contract

The WIT action surface now includes:

```wit
record profile-launch-request {
  profile-id: string,
  reason: option<string>,
}

variant plugin-action {
  request-profile-launch(profile-launch-request),
}
```

The Rust API mirrors this as
`PluginAction::RequestProfileLaunch(PluginProfileLaunchRequest)`.

## Host Policy

`PluginHost` validates and queues the request; it does not start SSH, resolve
credentials, read profile metadata for the plugin, or report launch status in
this slice.

Current policy:

- the plugin must have `profile-read`;
- `profile-id` is required;
- `profile-id` must be at most 256 bytes;
- `profile-id` must not contain whitespace or control characters;
- `reason` is optional;
- `reason` must be at most 256 bytes;
- `reason` must not contain control characters.

Accepted requests are stored as `PendingProfileLaunchRequest` with the
host-attached `source_plugin`. The plugin does not provide or override the
source plugin id.

Hosts can inspect pending requests with `profile_launch_requests()` and consume
them once with `take_profile_launch_requests()`. The take path clears the queue
so an event loop can apply trusted launch policy without replaying stale plugin
actions.

## Privacy Boundary

The request carries only the opaque `profile-id` chosen by the plugin. The host
does not return profile data to the plugin.

The action does not expose:

- profile names, descriptions, tags, targets, users, ports, paths, or SSH
  arguments;
- profile existence, launchability, or default-profile status;
- credential resolver or vault references;
- profile store path or raw profile-store content;
- launch success/failure details.

A future consumer must re-read the profile store and validate the profile id at
launch time. It must not trust the queued request as proof that the profile
exists or remains launchable.

## Fixture Coverage

The Wasm fixture includes `fixture.profile-launch`, which returns
`request-profile-launch` with profile id `prod` and a short reason. `witty-ui`
and `TerminalApp` tests cover Wasm adaptation, permission enforcement, queue
inspection, and one-shot queue draining.

## Host Review Helper

`witty-ui` also provides `review_profile_launch_requests()`, a pure host-side
helper for the future trusted UI consumer. It takes a current `ProfileStoreV1`
and pending launch requests, revalidates the store and request syntax, and
returns redacted `PendingProfileLaunchRequestReview` rows.

The review result includes:

- source plugin id;
- requested opaque profile id;
- optional reason;
- status: `Launchable`, `RequiresCredentialResolver`, or `NotFound`;
- profile name, tags, and default marker when the id exists.

The review result still does not expose target host, user, port, OpenSSH paths,
extra arguments, remote command, credential ids, or raw store content. It also
does not launch SSH. A launch consumer must re-read the store again at the point
of launch if there is any delay or user confirmation step.

## Host Resolution Helper

`resolve_profile_launch_request()` is the fail-closed companion for the actual
launch point. It takes a current `ProfileStoreV1` and one pending request,
revalidates the store plus request syntax, and returns a
`ResolvedProfileLaunchRequest` only when the profile exists and is launchable.

It returns an error when:

- the requested id is not found;
- the profile requires a credential resolver;
- the queued request has an unsafe id or reason;
- the current profile store fails validation.

The resolved result contains a cloned `SshProfile` for trusted host code. This
is intentionally not exposed through the plugin ABI and still does not start a
PTY or SSH process.

## Host PTY Config Helper

On native targets, `resolve_profile_launch_pty_config()` performs the same
fail-closed resolution and converts the launchable profile to a `LocalPtyConfig`
for `ssh`. It still does not spawn the PTY process. This gives native/app-owned
code a deterministic handoff point before a later UI policy decides whether to
replace the current session, open a new tab, or reject the request.

## TerminalApp Convenience

`TerminalApp` exposes non-mutating convenience methods over its pending queue:

- `review_pending_profile_launch_requests()`
- `resolve_pending_profile_launch_pty_config()` on native targets
- `resolve_pending_profile_launch_pty_configs()` on native targets

These methods use the current pending queue and a caller-provided
`ProfileStoreV1`. They do not drain the queue and do not start PTY/SSH, so UI
code can review, confirm, and only then call `take_profile_launch_requests()`.

`dismiss_pending_profile_launch_request()` removes one pending launch request by
queue index without resolving the requested profile id. This is the app-owned
cancellation path for trusted UI. It does not report rejection to the plugin,
read profile metadata, or start PTY/SSH.

The broader app-level `review_pending_profile_actions()` helper includes launch
requests as `PendingProfileActionReview::ProfileLaunch` rows with
`PendingProfileActionKey::profile_launch(index)`. The key is a current queue
index, so trusted UI should review again after any dismissal or confirmed drain.
`dismiss_pending_profile_action()` can consume the same key for cancellation.
On native targets, `take_resolved_pending_profile_action_pty_config()` can also
consume a `PendingProfileActionConfirmation::ProfileLaunch` built from the same
key.

On native targets, `take_resolved_profile_launch_pty_config()` adds a single
request confirmed-drain step by queue index. It resolves the selected request
first, removes only that request after it resolves to `LocalPtyConfig`, and
leaves the queue intact on any resolution error.

`take_resolved_profile_launch_pty_configs()` keeps the batch confirmed-drain
path. It resolves every pending request first; only if all requests resolve to
`LocalPtyConfig` does it clear the pending queue and return the configs. Any
resolution error leaves the queue intact for host UI error handling or retry.

## Verification

- `cargo test -p witty-plugin-api --quiet`
- `cargo test -p witty-plugin-wasm --quiet`
- `cargo test -p witty-ui --quiet`
- `cargo test -p witty-app --quiet`
