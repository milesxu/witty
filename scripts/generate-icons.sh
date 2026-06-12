#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE="${ROOT_DIR}/assets/icon.svg"
MAIN_PNG="${ROOT_DIR}/assets/icon.png"
ICON_DIR="${ROOT_DIR}/assets/icons"

if ! command -v ffmpeg >/dev/null 2>&1; then
  printf 'error: ffmpeg is required to rasterize %s\n' "${SOURCE}" >&2
  exit 1
fi

mkdir -p "${ICON_DIR}"

for size in 16 24 32 48 64 128 256 512; do
  ffmpeg -y -hide_banner -loglevel error \
    -i "${SOURCE}" \
    -vf "scale=${size}:${size}:flags=lanczos" \
    -frames:v 1 \
    -pix_fmt rgba \
    "${ICON_DIR}/icon_${size}x${size}.png"
done

ffmpeg -y -hide_banner -loglevel error \
  -i "${SOURCE}" \
  -vf "scale=256:256:flags=lanczos" \
  -frames:v 1 \
  -pix_fmt rgba \
  "${MAIN_PNG}"

if command -v iconutil >/dev/null 2>&1; then
  ICONSET_DIR="${ROOT_DIR}/target/witty.iconset"
  mkdir -p "${ICONSET_DIR}"

  ffmpeg -y -hide_banner -loglevel error -i "${SOURCE}" -vf "scale=16:16:flags=lanczos" -frames:v 1 -pix_fmt rgba "${ICONSET_DIR}/icon_16x16.png"
  ffmpeg -y -hide_banner -loglevel error -i "${SOURCE}" -vf "scale=32:32:flags=lanczos" -frames:v 1 -pix_fmt rgba "${ICONSET_DIR}/icon_16x16@2x.png"
  ffmpeg -y -hide_banner -loglevel error -i "${SOURCE}" -vf "scale=32:32:flags=lanczos" -frames:v 1 -pix_fmt rgba "${ICONSET_DIR}/icon_32x32.png"
  ffmpeg -y -hide_banner -loglevel error -i "${SOURCE}" -vf "scale=64:64:flags=lanczos" -frames:v 1 -pix_fmt rgba "${ICONSET_DIR}/icon_32x32@2x.png"
  ffmpeg -y -hide_banner -loglevel error -i "${SOURCE}" -vf "scale=128:128:flags=lanczos" -frames:v 1 -pix_fmt rgba "${ICONSET_DIR}/icon_128x128.png"
  ffmpeg -y -hide_banner -loglevel error -i "${SOURCE}" -vf "scale=256:256:flags=lanczos" -frames:v 1 -pix_fmt rgba "${ICONSET_DIR}/icon_128x128@2x.png"
  ffmpeg -y -hide_banner -loglevel error -i "${SOURCE}" -vf "scale=256:256:flags=lanczos" -frames:v 1 -pix_fmt rgba "${ICONSET_DIR}/icon_256x256.png"
  ffmpeg -y -hide_banner -loglevel error -i "${SOURCE}" -vf "scale=512:512:flags=lanczos" -frames:v 1 -pix_fmt rgba "${ICONSET_DIR}/icon_256x256@2x.png"
  ffmpeg -y -hide_banner -loglevel error -i "${SOURCE}" -vf "scale=512:512:flags=lanczos" -frames:v 1 -pix_fmt rgba "${ICONSET_DIR}/icon_512x512.png"
  ffmpeg -y -hide_banner -loglevel error -i "${SOURCE}" -vf "scale=1024:1024:flags=lanczos" -frames:v 1 -pix_fmt rgba "${ICONSET_DIR}/icon_512x512@2x.png"

  iconutil -c icns "${ICONSET_DIR}" -o "${ROOT_DIR}/assets/icon.icns"
  printf 'generated %s\n' "${ROOT_DIR}/assets/icon.icns"
fi

printf 'generated %s and %s/*.png\n' "${MAIN_PNG}" "${ICON_DIR}"
