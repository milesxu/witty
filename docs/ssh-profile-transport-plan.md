# SSH Profile Transport Plan

Updated: 2026-05-31

## Purpose

`m218-openssh-profile-transport-plan` started the SSH client line without
touching secrets, inventory sync, SFTP, or browser-selected remote execution.
The first implementation boundary is deliberately small:

```text
OpenSshProfile
  -> validated ssh argv/env model
  -> LocalPtyConfig
  -> future LocalPtyTransport spawn path
```

This lets native window mode, the browser gateway, and future profile storage
share one profile-to-command boundary before any UI or vault work lands.

## OpenSSH Adapter

`witty-transport::OpenSshProfile` is a serializable profile model with:

- `host`
- optional `user`
- optional `port`
- optional identity file path
- optional OpenSSH config file path
- optional jump host
- optional `TERM` value, defaulting to `xterm-256color`
- `request_tty`, defaulting to true and emitting `-tt`
- advanced `extra_args`
- optional remote command argv

On native targets, `OpenSshProfile::to_local_pty_config(size)` builds a
`LocalPtyConfig` with program `ssh`. Tests assert the exact argv/env output, for
example:

```text
ssh -tt -p 2222 -i /tmp/id_ed25519 -F /tmp/ssh_config \
  -J bastion -o ServerAliveInterval=30 alice@example.com \
  tmux new -A -s main
```

The builder validates destination atoms (`host`, `user`, jump host, and `TERM`)
for empty values, whitespace, and control characters. Arguments are passed as
process argv entries rather than shell strings.

## Native Smoke

`m219-native-openssh-smoke` adds a deterministic no-network OpenSSH smoke path:

```bash
cargo run -p witty-app -- --openssh-profile-smoke
```

Internally it builds an `OpenSshProfile` for `witty.invalid`, disables
forced TTY and `TERM`, uses `-F none` to avoid user config dependencies, and
adds `-G -o BatchMode=yes`. OpenSSH then prints the resolved config and exits
without opening a network connection.

The smoke verifies:

- `LocalPtyTransport` can spawn the generated `ssh` config
- OpenSSH exits with code `0`
- output contains `hostname witty.invalid`
- output contains `batchmode yes`

This exercises the real PTY spawn boundary while staying independent of remote
hosts, credentials, known-host state, and network availability.

## Product Profile Schema

`m220-profile-schema-plan` adds a product-facing schema above the low-level
OpenSSH adapter:

```text
SshProfile
  metadata: id, name, description, tags
  target: host, user, port, jump_host
  credential: default_agent | identity_file | vault_secret reference
  terminal: TERM, request_tty
  openssh advanced: config_file, extra_args, remote_command
  -> OpenSshProfile
```

This keeps user-facing profile metadata separate from transport launch details.
`SshProfile::to_openssh_profile()` converts supported credential references and
advanced options into `OpenSshProfile`, then the existing native path can turn
that into `LocalPtyConfig`.

Credential handling is intentionally reference-only:

- `default_agent` relies on OpenSSH agent/config behavior and adds no secret
  material to argv.
- `identity_file` stores only a path reference.
- `vault_secret` stores only a secret id and fails conversion until a future
  credential resolver is explicitly provided.

The schema has focused coverage for metadata/target/advanced-option mapping,
vault-secret rejection without a resolver, validation of product fields, and
JSON serialization that contains only credential references rather than
passphrase/private-key material.

## Browser Gateway Profile Launch

`m221-browser-gateway-profile-launch` wires the schema into the browser-backed
product path without giving JavaScript arbitrary process control.

The trusted native launcher accepts an explicit local profile file:

```bash
witty --web --ssh-profile-json ./profile.json
```

The launcher reads and validates the `SshProfile`, converts it to
`OpenSshProfile`, then converts that into a `LocalPtyConfig` for the gateway.
The browser still receives only the one-use session config fields it already
needed:

- protocol version
- gateway URL
- gateway token
- mouse selection policy
- scrollback line limit
- config expiry

The browser session config does not include profile id, host, user, SSH argv,
identity-file path, jump host, or remote command. The gateway receives a
trusted `LocalPtyConfig` template and applies the browser-reported terminal
size at spawn time, so resize negotiation still works.

For now, `--ssh-profile-json` is mutually exclusive with raw `--program` and
`--arg`. A later profile-store slice should replace ad hoc JSON path selection
with a versioned local store and a profile id selector.

## Profile Store Boundary

`m222-profile-store-file-plan` defines the local store that should replace
ad hoc profile JSON files for product use. See `profile-store-file-plan.md`.

The selected V1 direction is:

```text
ProfileStoreV1
  schema: 1
  app: witty-profiles
  profiles: Vec<SshProfile>
  default_profile_id: Option<String>
```

The store remains secret-free. It can persist `default_agent`,
`identity_file.path`, or opaque `vault_secret.secret_id` credential references,
but not private key bytes, passphrases, decrypted vault values, or agent tokens.

`m223-profile-store-types` implemented this pure schema/validation layer in
`witty-transport`. It exports `ProfileStoreV1`, validation constants,
`ProfileStoreValidation`, and `SshProfileLaunchability`. Validation rejects
unsupported schema/app values, duplicate ids, missing defaults, unknown
top-level fields, oversized JSON, and profile count/tag/OpenSSH argv limits.
Unresolved `vault_secret` profiles are retained as
`RequiresCredentialResolver` instead of being treated as launchable.

The future launcher path should select by id:

```bash
witty --web --profile-store <path> --ssh-profile-id prod
```

Native launcher code loads and validates the store, chooses a profile, converts
it to an OpenSSH-backed `LocalPtyConfig`, and passes only the trusted PTY config
to the gateway. Browser session JSON remains redacted.

`m224-profile-store-launcher-selection` implemented this by-id selection path.
It requires `--profile-store` and `--ssh-profile-id` together, keeps raw
`--program`/`--arg` mutually exclusive with SSH profile launch, and rejects
profiles marked `RequiresCredentialResolver`.

## Boundary Decisions

- Keep OpenSSH as the first remote shell adapter. It reuses the user's existing
  SSH agent, config, ProxyJump, certificates, FIDO keys, and platform-specific
  authentication behavior.
- Put the model in `witty-transport`, not `witty-app`, so both native and browser
  gateway launch paths can reuse it.
- Keep the type available on wasm for future profile display/editing, but only
  expose `to_local_pty_config` on native targets.
- Do not parse `~/.ssh/config` in this slice. OpenSSH remains the parser and
  policy engine for config files.
- Do not store passphrases, private keys, or resolved secrets in the profile.

## Security Notes

- `extra_args` is an expert escape hatch and should be gated in product UI.
- The browser must never choose arbitrary SSH argv over the gateway protocol.
  Profile selection belongs to the native launcher or a trusted profile store.
- A future profile vault should store metadata separately from secret material.
- Clipboard, OSC 52, shell integration output, and plugin events remain outside
  this SSH profile boundary.

## Follow-Up Queue

1. `m225-profile-store-atomic-write-plan`: implement or plan atomic
   write/permission behavior after read-only loading and launch selection are
   covered.

## Non-Goals

- SFTP file browser
- tunnel dashboard
- host inventory sync
- vault/keychain integration
- SSH config import UI
- remote reconnect/session resume
- replacing OpenSSH with `russh` or `ssh2`
