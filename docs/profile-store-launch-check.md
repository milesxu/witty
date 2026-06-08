# Profile Store Launch Check

Updated: 2026-06-01

## Purpose

`m326-profile-store-launch-check` adds a local-safe profile validation command:

```text
witty --profile-store-check-launch <id> [--profile-store <path>]
```

The command verifies that a stored SSH profile can be converted into the
trusted OpenSSH launch boundary without starting SSH, launching the browser,
opening a native window, creating a `wgpu` surface, or touching Vulkan.

## Behavior

- Reads the explicit profile store path, or the platform default profile store
  path when `--profile-store` is omitted.
- Treats a missing default store like the list command: an empty store is used
  and the requested id fails as not found.
- Keeps explicit missing store paths as hard errors.
- Fails when the requested profile id does not exist.
- Fails when the profile requires a future credential resolver.
- Converts launchable profiles through `SshProfile::to_openssh_profile()` to
  exercise the same validation path used by launcher selection.

## Redaction Boundary

Successful output is deliberately aggregate-only:

```text
profile launch check: id=prod launchability=launchable default=true request_tty=true term=set identity_file=true config_file=true jump_host=true extra_args=1 remote_command_args=1
```

It does not print:

- target host
- user
- port
- jump host value
- identity-file path
- config-file path
- OpenSSH extra arguments
- remote command arguments
- credential secret ids

This makes the command suitable for local diagnostics and CI logs on the
Linux/M1000 machine while browser/WebGPU and real-window validation are
suspended.

## Verification

- `cargo test -p witty-app profile_store_cli --quiet`
- `cargo test -p witty-app --quiet`
- `cargo check -p witty-app --quiet`
- `cargo test -p witty-transport profile_store --quiet`
- `cargo test -p witty-launcher profile_store --quiet`
- `git diff --check`
