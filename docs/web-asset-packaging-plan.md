# Web Asset Packaging Plan

Updated: 2026-05-30

## Purpose

`witty --web` currently depends on `--web-root target/witty-web-smoke`.
That is acceptable for browser smoke tests, but it is not a product packaging
model. This plan defines a narrow path from the current smoke bundle to a
repeatable browser UI asset bundle.

## Current State

Current build script:

```text
scripts/build-witty-web-smoke.sh
```

Current output:

```text
target/witty-web-smoke/
  index.html
  smoke.js
  pkg/
    witty_web.js
    witty_web_bg.wasm
    witty_web.d.ts
    witty_web_bg.wasm.d.ts
  fonts/
    witty-mono.ttf
```

Current launcher default:

```text
LauncherConfig::default().web_root = target/witty-web-smoke
```

Current launcher allowlist:

```text
/
/index.html
/smoke.js
/pkg/witty_web.js
/pkg/witty_web_bg.wasm
/fonts/witty-mono.ttf
/session/<id>.json
```

Problems:

- `smoke.js` is both the product browser entry and the test hook surface.
- The default web root points to a generated `target/` directory.
- The font is copied from the local machine, so product output is not
  reproducible.
- There is no manifest describing asset names, hashes, protocol version, or
  build metadata.
- The launcher serves a fixed smoke asset allowlist, not a product asset set.

## Decision

Use an external product asset directory first. Do not embed wasm/static assets
inside `witty` yet.

Rationale:

- The wasm bundle is currently around 2.5 MiB before compression; embedding it
  into the native binary can wait until the app shape stabilizes.
- External assets are easier to inspect, diff, hash, and package into Linux
  distro paths, archives, AppImage, or future installers.
- The launcher already has a `--web-root` boundary; hardening that boundary is
  smaller than adding binary embedding plus content-type routing now.

Target product output:

```text
target/witty-web-dist/
  index.html
  app.js
  asset-manifest.json
  pkg/
    witty_web.js
    witty_web_bg.wasm
  fonts/
    witty-mono.ttf
  licenses/
    fonts.txt
```

Smoke output can keep extra Playwright-only artifacts such as
`smoke-canvas.png`; product dist should not include generated screenshots or
`.d.ts` files unless a downstream packaging task explicitly needs them.

## Entry Split

Create a browser entry split:

```text
crates/witty-web/static/index.html
crates/witty-web/static/app.js
crates/witty-web/static/smoke-hooks.js   # optional later split
```

`app.js` should own the real browser session behavior:

- load `pkg/witty_web.js`
- load font
- create wasm session
- fetch `/session/<id>.json` when launched by native launcher
- connect to gateway
- route keyboard input and resize

Smoke-only JavaScript hooks can remain exported on `window` for now, but the
file should be named product-first. A later cleanup can move Playwright helpers
behind `window.wittySmoke` or a separate `smoke-hooks.js` file.

## Build Script Shape

Add a product build script:

```text
scripts/build-witty-web-dist.sh
```

Responsibilities:

1. Build `witty-web` for `wasm32-unknown-unknown --release`.
2. Run `wasm-bindgen --target web`.
3. Copy static product files into `target/witty-web-dist`.
4. Copy a deterministic bundled monospace font into `fonts/`.
5. Write `asset-manifest.json`.
6. Validate required files exist and are non-empty.

Recommended options:

```text
WITTY_WEB_DIST_DIR=target/witty-web-dist
WITTY_WEB_FONT=/path/to/font.ttf
WITTY_WEB_FONT_LICENSE=/path/to/license.txt
```

`WITTY_WEB_FONT` should override the bundled/default font for development,
but release packaging should use a repository-controlled font file with license
metadata.

Keep `scripts/build-witty-web-smoke.sh` as a wrapper or sibling:

- It may call `build-witty-web-dist.sh`.
- It can copy/rename `app.js` into the smoke output or serve dist directly.
- It may keep Playwright screenshot output under `target/witty-web-smoke`.

## Manifest

Add `asset-manifest.json` to the dist output.

Proposed shape:

```json
{
  "schema": 1,
  "app": "witty-web",
  "protocol": 1,
  "generated_by": "scripts/build-witty-web-dist.sh",
  "assets": [
    {
      "path": "index.html",
      "content_type": "text/html; charset=utf-8",
      "sha256": "<hex>",
      "bytes": 1234
    },
    {
      "path": "app.js",
      "content_type": "text/javascript; charset=utf-8",
      "sha256": "<hex>",
      "bytes": 1234
    },
    {
      "path": "pkg/witty_web.js",
      "content_type": "text/javascript; charset=utf-8",
      "sha256": "<hex>",
      "bytes": 1234
    },
    {
      "path": "pkg/witty_web_bg.wasm",
      "content_type": "application/wasm",
      "sha256": "<hex>",
      "bytes": 1234
    },
    {
      "path": "fonts/witty-mono.ttf",
      "content_type": "font/ttf",
      "sha256": "<hex>",
      "bytes": 1234
    }
  ]
}
```

The manifest gives the launcher and packaging scripts a deterministic asset
surface without needing a general static file server.

## Launcher Resolution

Change launcher web root resolution in a later implementation milestone:

```text
explicit --web-root
  -> WITTY_WEB_ROOT
  -> installed web asset dir next to executable
  -> development fallback target/witty-web-dist
```

Suggested installed path conventions:

- Linux package: `/usr/share/witty/web`
- Portable archive: `<install-root>/share/witty/web`
- Local development: `<repo>/target/witty-web-dist`

The launcher should continue to validate that the resolved web root exists and
contains a valid manifest.

## HTTP Serving Boundary

The launcher should serve only files listed in the manifest plus:

```text
/session/<id>.json
```

Rules:

- normalize request paths before lookup
- reject path traversal
- reject unknown assets with `404`
- preserve `Cache-Control: no-store` for session config
- product assets may use cache headers once hashed filenames exist, but v1 can
  keep conservative cache behavior

This keeps the current explicit allowlist property while removing hardcoded
smoke filenames from launcher code.

## Font Packaging

The current smoke build copies the first local monospace font it finds. Product
packaging should not rely on host fonts.

Plan:

1. Pick one repository-controlled monospace font asset.
2. Store the font under a dedicated asset directory with its license.
3. Copy it into `target/witty-web-dist/fonts/witty-mono.ttf`.
4. Add the font and license file to `asset-manifest.json`.

The initial repository-controlled font is DejaVu Sans Mono. Keep
`WITTY_WEB_FONT` and `WITTY_WEB_FONT_LICENSE` for local override builds.

## Verification

Build checks:

```text
scripts/build-witty-web-dist.sh
node --check target/witty-web-dist/app.js
test -s target/witty-web-dist/pkg/witty_web_bg.wasm
test -s target/witty-web-dist/asset-manifest.json
```

Rust checks:

```text
cargo check --workspace
cargo test -p witty-launcher -p witty-app
```

Browser smoke:

```text
WITTY_WEB_SMOKE_GATEWAY=launcher \
WITTY_WEB_ROOT=target/witty-web-dist \
scripts/run-witty-web-smoke.sh
```

The smoke should verify:

- page loads from product dist
- session config remains one-use
- gateway connects
- keyboard input reaches PTY
- canvas screenshot is nonblank
- launcher exits cleanly after browser close

## Implementation Milestones

Recommended next tasks:

1. `m69-web-dist-builder`
   - done after this plan: added `scripts/build-witty-web-dist.sh`, created
     `app.js`, emitted `asset-manifest.json`, and kept existing smoke passing

2. `m70-launcher-manifest-serving`
   - done after this plan: launcher loads and validates
     `asset-manifest.json`, serves only manifest-listed assets, and has tests
     for path traversal, unknown assets, and byte-count mismatches

3. `m71-web-root-resolution`
   - done after this plan: launcher resolves web assets from `--web-root`,
     `WITTY_WEB_ROOT`, installed `share/witty/web` candidates, then
     `target/witty-web-dist`, and README documents the default flow

4. `m72-bundled-font-selection`
   - done after this plan: selected DejaVu Sans Mono, added it under
     `crates/witty-web/assets/fonts/witty-mono.ttf`, copied license metadata
     into the dist output, and kept manifest coverage for font/license assets

## Non-Goals For This Step

- Embedding web assets into the native binary.
- Adding hashed filenames or long-lived cache headers.
- Adding service workers.
- Supporting remote hosted web UI.
- Multi-session asset serving.
