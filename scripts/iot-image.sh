#!/usr/bin/env bash
set -euo pipefail

# Produce per-board flash artifacts in dist/, matching the release CI artifact
# names exactly (reusable-iot-build.yml) so a local run can substitute for a
# failed Action: an ELF copy plus a whole-flash merged image via
# `espflash save-image --merge`, both tagged with the version from version.sh
# (mirrors CI's APP_VERSION/dev version). Needs the release ELF, i.e.
# `moon run iot:build`/`iot:build-s3` first.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
version=$(bash "$ROOT/scripts/version.sh" "$ROOT/apps/iot/Cargo.toml")

gen_image() {
  local board=$1 chip=$2 target=$3
  local src="$ROOT/apps/iot/target/$target/release/vanling"
  [ -f "$src" ] || {
    echo "missing $src (run the matching build task first)" >&2
    exit 1
  }
  mkdir -p "$ROOT/dist"
  local elf="$ROOT/dist/vanling-iot-$board-$version-$target.elf"
  local bin="$ROOT/dist/vanling-iot-$board-$version-merged.bin"
  cp "$src" "$elf"
  espflash save-image --chip "$chip" --merge "$elf" "$bin"
}

BOARD_ESP32C6="esp32c6-devkitc-1 esp32c6 riscv32imac-unknown-none-elf"
BOARD_ESP32S3="lckfb-szpi-esp32s3 esp32s3 xtensa-esp32s3-none-elf"

if [ $# -eq 0 ]; then
  gen_image $BOARD_ESP32C6
  gen_image $BOARD_ESP32S3
else
  for spec in "$@"; do
    case "$spec" in
      esp32c6-devkitc-1) gen_image $BOARD_ESP32C6 ;;
      lckfb-szpi-esp32s3) gen_image $BOARD_ESP32S3 ;;
      *) echo "unknown board: $spec (esp32c6-devkitc-1 | lckfb-szpi-esp32s3)" >&2; exit 1 ;;
    esac
  done
fi