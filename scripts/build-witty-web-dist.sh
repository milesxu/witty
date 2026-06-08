#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${WITTY_WEB_DIST_DIR:-${ROOT_DIR}/target/witty-web-dist}"
PKG_DIR="${OUT_DIR}/pkg"
FONT_DIR="${OUT_DIR}/fonts"
LICENSE_DIR="${OUT_DIR}/licenses"
STATIC_DIR="${ROOT_DIR}/crates/witty-web/static"
BUNDLED_FONT="${ROOT_DIR}/crates/witty-web/assets/fonts/witty-mono.ttf"
BUNDLED_FONT_LICENSE="${ROOT_DIR}/crates/witty-web/assets/licenses/dejavu-fonts.txt"
WASM_IN="${ROOT_DIR}/target/wasm32-unknown-unknown/release/witty_web.wasm"
FONT_IN="${WITTY_WEB_FONT:-${WITTY_WEB_SMOKE_FONT:-}}"
FONT_LICENSE_IN="${WITTY_WEB_FONT_LICENSE:-}"

if ! command -v wasm-bindgen >/dev/null 2>&1; then
  cat >&2 <<'MSG'
missing wasm-bindgen CLI

Install the CLI version matching Cargo.lock's wasm-bindgen crate, then rerun:
  cargo install wasm-bindgen-cli --version 0.2.122 --locked
MSG
  exit 2
fi

cargo build -p witty-web --target wasm32-unknown-unknown --release

if [[ -z "${FONT_IN}" ]]; then
  FONT_IN="${BUNDLED_FONT}"
  FONT_LICENSE_IN="${BUNDLED_FONT_LICENSE}"
fi

if [[ -z "${FONT_IN}" || ! -r "${FONT_IN}" ]]; then
  cat >&2 <<'MSG'
missing web font

Expected the bundled repo font or point WITTY_WEB_FONT at a local
monospace TTF/OTF:
  WITTY_WEB_FONT=/path/to/monospace.ttf scripts/build-witty-web-dist.sh
MSG
  exit 2
fi

if [[ -n "${FONT_LICENSE_IN}" && ! -r "${FONT_LICENSE_IN}" ]]; then
  echo "missing web font license metadata: ${FONT_LICENSE_IN}" >&2
  exit 2
fi

mkdir -p "${PKG_DIR}" "${FONT_DIR}" "${LICENSE_DIR}"
cp "${STATIC_DIR}/index.html" "${OUT_DIR}/index.html"
cp "${STATIC_DIR}/app.js" "${OUT_DIR}/app.js"
cp "${FONT_IN}" "${FONT_DIR}/witty-mono.ttf"

wasm-bindgen \
  "${WASM_IN}" \
  --target web \
  --no-typescript \
  --out-dir "${PKG_DIR}" \
  --out-name witty_web

if [[ -n "${FONT_LICENSE_IN}" ]]; then
  cp "${FONT_LICENSE_IN}" "${LICENSE_DIR}/fonts.txt"
else
  cat > "${LICENSE_DIR}/fonts.txt" <<MSG
Witty local web build font source:
${FONT_IN}

WITTY_WEB_FONT_LICENSE was not provided for this override font.
Release packaging must include license metadata for redistributed artifacts.
MSG
fi

required_files=(
  "index.html"
  "app.js"
  "pkg/witty_web.js"
  "pkg/witty_web_bg.wasm"
  "fonts/witty-mono.ttf"
  "licenses/fonts.txt"
)

for path in "${required_files[@]}"; do
  if [[ ! -s "${OUT_DIR}/${path}" ]]; then
    echo "missing or empty web dist asset: ${OUT_DIR}/${path}" >&2
    exit 1
  fi
done

asset_entry() {
  local path="$1"
  local content_type="$2"
  local file="${OUT_DIR}/${path}"
  local sha
  local bytes
  sha="$(sha256sum "${file}" | awk '{print $1}')"
  bytes="$(wc -c < "${file}" | tr -d ' ')"
  printf '    {"path":"%s","content_type":"%s","sha256":"%s","bytes":%s}' \
    "${path}" "${content_type}" "${sha}" "${bytes}"
}

{
  echo '{'
  echo '  "schema": 1,'
  echo '  "app": "witty-web",'
  echo '  "protocol": 1,'
  echo '  "generated_by": "scripts/build-witty-web-dist.sh",'
  echo '  "assets": ['
  asset_entry "index.html" "text/html; charset=utf-8"
  echo ','
  asset_entry "app.js" "text/javascript; charset=utf-8"
  echo ','
  asset_entry "pkg/witty_web.js" "text/javascript; charset=utf-8"
  echo ','
  asset_entry "pkg/witty_web_bg.wasm" "application/wasm"
  echo ','
  asset_entry "fonts/witty-mono.ttf" "font/ttf"
  echo ','
  asset_entry "licenses/fonts.txt" "text/plain; charset=utf-8"
  echo
  echo '  ]'
  echo '}'
} > "${OUT_DIR}/asset-manifest.json"

if [[ ! -s "${OUT_DIR}/asset-manifest.json" ]]; then
  echo "missing generated web dist manifest" >&2
  exit 1
fi

echo "Witty web dist built at ${OUT_DIR}"
