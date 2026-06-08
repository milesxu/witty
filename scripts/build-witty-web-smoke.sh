#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${ROOT_DIR}/target/witty-web-smoke"

WITTY_WEB_DIST_DIR="${OUT_DIR}" "${ROOT_DIR}/scripts/build-witty-web-dist.sh"
echo "Witty web smoke built at ${OUT_DIR}"
