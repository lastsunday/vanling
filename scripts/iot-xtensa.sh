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
# Build-time configuration that has to reach the compiler, not the build script's
# shell: `option_env!` is expanded where the crate is compiled, so a variable set here
# is only seen if the compiler runs with it. Listed by name because forwarding the whole
# environment would forward whatever the host happens to export, and a stray one would
# silently become a build input.
FORWARDED_ENV=(IOT_BOOT_PAGE IOT_DEMAND IOT_WIFI_SSID IOT_WIFI_PSK)

# This part's data cache, halved, and the reason it lives here rather than in
# `apps/iot/.cargo/config.toml` next to the other esp-hal settings.
#
# esp-hal's S3 linker leaves the DRAM above `dram_seg` to the heap but reserves the top
# of it for the data cache, so with the default 64 KB cache the reclaimed region is
# empty and the Wi-Fi driver's heap has nowhere to sit that does not come out of the
# main stack — which is asserted at 64 KB and has about 12 KB of slack. Halving the
# cache buys 32 KB back, and `crates/chip-esp/src/radio/` is the budget that depends
# on it.
#
# It cannot be a `[env]` entry: esp-config treats an option that is not active for the
# chip being built as an unknown one and fails the build outright, and
# `data-cache-size` is active on the S2 and the S3 only. This wrapper is the one place
# every S3 build passes through, so setting it here reaches exactly those builds — and
# "exactly" is why it is keyed off the target rather than exported outright: this script
# also runs the C6, where the same variable stops the build dead. The cost is cache the
# panel and camera DMA run without; `@/records/iot/wifi.md` has what was re-measured
# after the change.
ESP_S3_CONFIG_ENV=(ESP_HAL_CONFIG_DATA_CACHE_SIZE=32KB)

# Whether these arguments build for the S3, from the `--target` they carry.
targets_s3() {
  local arg previous="" target=""
  for arg in "$@"; do
    case "$previous" in
      --target) target="$arg" ;;
    esac
    case "$arg" in
      --target=*) target="${arg#--target=}" ;;
    esac
    previous="$arg"
  done
  case "$target" in
    xtensa-esp32s3*) return 0 ;;
    *) return 1 ;;
  esac
}

if targets_s3 "$@"; then
  for setting in "${ESP_S3_CONFIG_ENV[@]}"; do
    export "$setting"
  done
fi

docker_env_args() {
  local arg
  for arg in "${FORWARDED_ENV[@]}"; do
    if [ -n "${!arg:-}" ]; then
      printf -- '-e\n%s=%s\n' "$arg" "${!arg}"
    fi
  done
  # Unconditional for this chip, which have no "unset" shape to test for.
  if targets_s3 "$@"; then
    for arg in "${ESP_S3_CONFIG_ENV[@]}"; do
      printf -- '-e\n%s\n' "$arg"
    done
  fi
}

run_docker() {
  local log status
  log="$(mktemp "${TMPDIR:-/tmp}/iot-xtensa.XXXXXX")"
  set +e
  docker run --rm \
    -v "$PROJECT:/project" \
    -v "$HOME/.cargo/registry:/home/esp/.cargo/registry" \
    -w /project \
    $(docker_env_args "$@") \
    "$IMAGE" \
    bash -lc 'source /home/esp/export-esp.sh && cargo --offline "$@"' \
    bash "$@" 2>&1 | tee "$log"
  status=${PIPESTATUS[0]}
  set -e
  # Two shapes of "the offline cache is missing something this build needs": a lock
  # that pins a version the cache has never seen, and a dependency that is not in the
  # lock at all — which is what adding a crate to a workspace looks like from inside
  # the container, because offline resolution cannot add packages. Both are the
  # cache's state rather than the build's, so both get the one online retry.
  if [ "$status" -eq 0 ] ||
     ! rg -q 'but --offline was specified|no matching package named|failed to select a version|unable to get packages from source' "$log"; then
    rm -f "$log"
    return "$status"
  fi
  rm -f "$log"
  echo "[iot-xtensa] offline cache incomplete; bootstrapping once online (subsequent builds stay offline)" >&2
  docker run --rm \
    -v "$PROJECT:/project" \
    -v "$HOME/.cargo/registry:/home/esp/.cargo/registry" \
    -w /project \
    $(docker_env_args "$@") \
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