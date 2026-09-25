#!/usr/bin/env bash
set -euo pipefail

# Runs a cargo command for the xtensa-esp32s3 target, dispatching by
# environment so one moon task serves both CI/release and macOS (Intel):
#
#   - native: a complete espup "esp" toolchain (pinned to 1.95.0.0, matching
#     the Docker image below) is available -> run cargo directly. Handles both
#     the unified xtensa-esp-elf layout (espup 0.17+) and the legacy per-chip
#     xtensa-esp32s3-elf layout. The esp fork is a nightly build, so the
#     -Z build-std flags in the moon tasks work here; it is invoked by its
#     absolute path because a plain `cargo` in this nix shell resolves to the
#     stable toolchain, which rejects -Z;
#   - docker: esp toolchain missing/incomplete (e.g. macOS Intel, where esp-rs
#     stopped shipping toolchains at v1.91+) -> fall back to the
#     espressif/idf-rust container;
#   - otherwise: fail with a hint instead of guessing.
#
# Args are forwarded verbatim to cargo (e.g. `check -p iot-app --target ...`).

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECT="$ROOT/apps/iot"
IMAGE="espressif/idf-rust:esp32s3_1.95.0.0"
ESP_DIR="${ESPUP_TOOLCHAIN_DIR:-$HOME/.rustup/toolchains/esp}"

# Resolves the bin dir of the esp GCC linked into a complete espup install.
# Works with both the unified xtensa-esp-elf layout (espup 0.17+, nested as
# xtensa-esp-elf/esp-<ver>/xtensa-esp-elf/bin) and the legacy per-chip layout
# (xtensa-esp32s3-elf/esp-<ver>/xtensa-esp32s3-elf/bin).
find_gcc_bin() {
  local gcc
  gcc="$(find "$ESP_DIR" -type f -name 'xtensa-esp*-elf-gcc' -print -quit 2>/dev/null || true)"
  if [ -n "$gcc" ]; then dirname "$gcc"; fi
}

# Only a complete espup install counts as native: besides rustc and its cargo
# we need a GCC linker and the rust-src component for -Z build-std.
has_native_esp() {
  [ -x "$ESP_DIR/bin/rustc" ] &&
    [ -x "$ESP_DIR/bin/cargo" ] &&
    [ -n "$(find_gcc_bin)" ] &&
    [ -d "$ESP_DIR/lib/rustlib/src/rust/library" ]
}

has_docker() {
  command -v docker >/dev/null 2>&1
}

run_native() {
  [ -f "$HOME/export-esp.sh" ] && . "$HOME/export-esp.sh"
  [ -f "$ESP_DIR/export-esp.sh" ] && . "$ESP_DIR/export-esp.sh"
  local gcc_bin
  gcc_bin="$(find_gcc_bin)"
  export PATH="$ESP_DIR/bin${gcc_bin:+:$gcc_bin}:$PATH"
  export RUSTUP_TOOLCHAIN=esp
  cd "$PROJECT"
  "$ESP_DIR/bin/cargo" "$@"
}

# Runs cargo offline in the pinned image: both the project Cargo.lock and the
# esp toolchain's own std-lock fix the exact versions, so artifact bytes are
# reproducible. On a fresh machine the shared cache lacks crates that the
# -Z build-std std-lock needs (e.g. memchr 2.7.6); that offline-only failure is
# detected and retried once online, which downloads the same pinned versions
# cargo already resolved — subsequent builds stay offline and byte-identical.
run_docker() {
  local log status
  log="$(mktemp "${TMPDIR:-/tmp}/iot-xtensa.XXXXXX")"
  set +e
  docker run --rm \
    -v "$PROJECT:/project" \
    -v "$HOME/.cargo/registry:/home/esp/.cargo/registry" \
    -w /project \
    "$IMAGE" \
    bash -lc 'source /home/esp/export-esp.sh && cargo --offline "$@"' \
    bash "$@" 2>&1 | tee "$log"
  status=${PIPESTATUS[0]}
  set -e
  if [ "$status" -eq 0 ] ||
     ! rg -q 'but --offline was specified' "$log"; then
    rm -f "$log"
    return "$status"
  fi
  rm -f "$log"
  echo "[iot-xtensa] offline cache incomplete; bootstrapping once online (subsequent builds stay offline)" >&2
  docker run --rm \
    -v "$PROJECT:/project" \
    -v "$HOME/.cargo/registry:/home/esp/.cargo/registry" \
    -w /project \
    "$IMAGE" \
    bash -lc 'source /home/esp/export-esp.sh && cargo "$@"' \
    bash "$@"
}

if has_native_esp; then
  echo "[iot-xtensa] native esp toolchain" >&2
  run_native "$@"
elif has_docker; then
  echo "[iot-xtensa] docker fallback ($IMAGE)" >&2
  run_docker "$@"
else
  echo "no esp toolchain (espup install -n esp -v 1.95.0.0) nor docker available" >&2
  exit 1
fi