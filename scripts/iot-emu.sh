#!/usr/bin/env bash
set -euo pipefail

# Run the C6 firmware under esp-emu and assert the boot log appears.
#
# Phase A validated: esp-emu boots the bare-metal ESP32-C6 image through the
# real ROM + IDF 2nd-stage bootloader into the embassy executor, and UART0
# console output is captured by --exit-on. The firmware must be built with the
# esp-println UART printer (not `auto`, whose USB-Serial-JTAG fallback is not
# forwarded by esp-emu; see docs/content/development/iot/emulation.md).
#
# Usage: scripts/iot-emu.sh [merged.bin] [timeout]
#   merged.bin   path to the esp32c6 merged flash image (default: latest from dist/)
#   timeout      esp-emu --timeout duration (default: 10s)
#
# Runs natively on Linux (CI) and through docker linux/amd64 on macOS (Intel).

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CACHE_DIR="${HOME}/.cache/vanling/esp-emu"
EMU_VERSION="0.42.0"
EMU_BIN="$CACHE_DIR/esp-emu"
TIMEOUT="${2:-10s}"
EXIT_ON='[IOT] boot ok'

# ---- 1. ensure esp-emu binary is cached -------------------------------------
ensure_emu() {
  [ -x "$EMU_BIN" ] && return 0
  echo "[iot-emu] downloading esp-emu $EMU_VERSION (linux x86_64)" >&2
  mkdir -p "$CACHE_DIR"
  curl -fsSL "https://github.com/espressif/esp-emulator/releases/download/v${EMU_VERSION}/esp-emu-${EMU_VERSION}-x86_64-unknown-linux-gnu.tar.gz" \
    | tar xz -C "$CACHE_DIR" --strip-components=1
  [ -x "$EMU_BIN" ] || { echo "[iot-emu] esp-emu binary not found after install" >&2; exit 1; }
}

# ---- 2. resolve merged.bin ----------------------------------------------------
find_merged() {
  local path="$1"
  if [ -n "$path" ] && [ -f "$path" ]; then
    echo "$(cd "$(dirname "$path")" && pwd)/$(basename "$path")"
    return
  fi
  local found
  found=$(find "$ROOT/dist" \
    -name '*esp32c6*-emu-merged.bin' -type f 2>/dev/null \
    | sort -r | head -1)
  [ -n "$found" ] || {
    found=$(find "$ROOT/dist" "$ROOT/apps/iot/target" \
      -name '*esp32c6*-merged.bin' -type f 2>/dev/null \
      | sort -r | head -1)
  }
  [ -n "$found" ] || { echo "[iot-emu] no merged.bin found (run moon run iot:build-c6-emu)" >&2; exit 1; }
  echo "$(cd "$(dirname "$found")" && pwd)/$(basename "$found")"
}

# ---- 3. run esp-emu, streaming output through a capture file ------------------
run_emu() {
  local merged="$1" out="$2"
  case "$(uname -s)" in
    Linux)
      "$EMU_BIN" \
        --chip esp32c6 \
        --firmware "$merged" \
        --exit-on "$EXIT_ON" \
        --timeout "$TIMEOUT" 2>&1 | tee "$out"
      ;;
    Darwin)
      docker run --rm --platform linux/amd64 \
        -v "$EMU_BIN:/usr/local/bin/esp-emu:ro" \
        -v "$merged:/work/merged.bin:ro" \
        --workdir /work \
        ubuntu:24.04 \
        esp-emu \
          --chip esp32c6 \
          --firmware /work/merged.bin \
          --exit-on "$EXIT_ON" \
          --timeout "$TIMEOUT" 2>&1 | tee "$out"
      ;;
    *)
      echo "[iot-emu] unsupported host: $(uname -s) (need Linux or macOS with docker)" >&2
      exit 1
      ;;
  esac
  return "${PIPESTATUS[0]}"
}

# ---- main --------------------------------------------------------------------
ensure_emu
MERGED_BIN="$(find_merged "${1:-}")"
OUT="$(mktemp)"
echo "[iot-emu] running esp-emu v$EMU_VERSION on $MERGED_BIN (timeout=$TIMEOUT, exit-on='$EXIT_ON')" >&2
if run_emu "$MERGED_BIN" "$OUT"; then
  if grep -Fq "$EXIT_ON" "$OUT"; then
    echo "[iot-emu] PASS: '$EXIT_ON' detected" >&2
    rm -f "$OUT"
    exit 0
  fi
  echo "[iot-emu] FAIL: esp-emu exited 0 but '$EXIT_ON' not in output (timeout or hang)" >&2
  rm -f "$OUT"
  exit 1
else
  echo "[iot-emu] FAIL: esp-emu returned $? (crash or timeout)" >&2
  rm -f "$OUT"
  exit 1
fi