# Web Root Resolution

Updated: 2026-05-30

## Purpose

`witty --web` can now run from the product dist directory without passing
`--web-root` in the common local development case.

## Resolution Order

The launcher resolves browser assets in this order:

1. Explicit `--web-root <path>`
2. `WITTY_WEB_ROOT`
3. Installed asset directory derived from the executable path
4. Development fallback `target/witty-web-dist`

Installed candidates are:

```text
<exe-dir>/share/witty/web
<exe-dir>/../share/witty/web
```

The second form covers conventional package layouts such as
`/usr/bin/witty` plus `/usr/share/witty/web`, and portable archives with
`bin/witty` plus `share/witty/web`.

## Validation

Resolution only chooses a path. Startup still validates that the chosen web root
exists and contains a valid `asset-manifest.json`. To prepare the local
development fallback:

```text
scripts/build-witty-web-dist.sh
cargo run -p witty-app -- --web
```

Use `--web-root` for smoke output or ad hoc builds, and use
`WITTY_WEB_ROOT` for shell/session-level overrides.
