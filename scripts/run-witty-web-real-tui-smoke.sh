#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PLAYWRIGHT_ROOT="${WITTY_PLAYWRIGHT_ROOT:-${ROOT_DIR}/target/witty-web-smoke-tools}"
WITTY_CHROMIUM_EXECUTABLE="${WITTY_CHROMIUM_EXECUTABLE:-}"
LOCAL_OPENGL_ONLY_MARKER="${ROOT_DIR}/.witty-local-opengl-only"

if [[ -f "${LOCAL_OPENGL_ONLY_MARKER}" && "${WITTY_ALLOW_LOCAL_CHROMIUM_SMOKE:-}" != "1" ]]; then
  cat >&2 <<'MSG'
Witty browser real-TUI smoke is disabled on this machine by .witty-local-opengl-only.
This Linux/M1000 host is reserved for native wgpu OpenGL backend development.
To run Chromium/WebGPU smoke deliberately, set WITTY_ALLOW_LOCAL_CHROMIUM_SMOKE=1.
MSG
  exit 2
fi

"${ROOT_DIR}/scripts/build-witty-web-smoke.sh"

if ! command -v node >/dev/null 2>&1; then
  echo "missing node; cannot run Playwright real-TUI smoke" >&2
  exit 2
fi

if ! WITTY_PLAYWRIGHT_ROOT="${PLAYWRIGHT_ROOT}" node --input-type=module <<'JS' >/dev/null 2>&1
import { createRequire } from "node:module";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const roots = [
  process.cwd(),
  process.env.WITTY_PLAYWRIGHT_ROOT,
].filter(Boolean);

for (const root of roots) {
  try {
    createRequire(pathToFileURL(join(root, "package.json")))("playwright");
    process.exit(0);
  } catch {
    // Try the next root.
  }
}

process.exit(1);
JS
then
  cat >&2 <<'MSG'
missing Playwright node package

Install it in the isolated smoke-tool directory, then rerun:
  npm install --prefix target/witty-web-smoke-tools --no-save playwright
  target/witty-web-smoke-tools/node_modules/.bin/playwright install chromium
MSG
  exit 2
fi

if [[ -z "${WITTY_CHROMIUM_EXECUTABLE}" && -d "${HOME}/.cache/ms-playwright" ]]; then
  while IFS= read -r candidate; do
    WITTY_CHROMIUM_EXECUTABLE="${candidate}"
  done < <(find "${HOME}/.cache/ms-playwright" -path '*/chrome-linux/chrome' -type f 2>/dev/null | sort)
fi

export WITTY_CHROMIUM_EXECUTABLE
WITTY_PLAYWRIGHT_ROOT="${PLAYWRIGHT_ROOT}" node "${ROOT_DIR}/scripts/run-witty-web-real-tui-smoke.mjs"
