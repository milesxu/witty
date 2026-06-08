# Launcher Manifest Serving

Updated: 2026-05-30

## Purpose

`witty --web` now treats `asset-manifest.json` as the source of truth for
browser UI assets. The launcher no longer serves a hardcoded list of product
and smoke filenames.

## Load Contract

At startup, the launcher reads:

```text
<web-root>/asset-manifest.json
```

The manifest must use schema `1`, app `witty-web`, and the browser gateway
protocol version supported by the launcher. Every listed asset must:

- use a relative path made only of normal path components
- stay inside the web root
- include a non-empty content type without newline characters
- include a 64-character hex sha256 field
- exist as a regular file
- match the manifest byte count

The manifest must include `index.html`. The URL `/` maps to that asset.

## Serving Contract

Only manifest-listed assets are served. Unknown paths, traversal attempts, and
legacy smoke-only paths that are not in the manifest return `404`.

Session config URLs remain dynamic and are not manifest assets:

```text
/session/<id>.json
```

Those responses stay one-use and no-store.

## Verification

Unit tests cover manifest loading, unknown asset rejection, traversal rejection,
and byte-count validation. The launcher browser smoke covers the generated dist
manifest from `scripts/build-witty-web-smoke.sh`.
