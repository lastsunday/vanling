#!/usr/bin/env bash
# Checks that every component and virtual component builds on its own.
#
# `features.md` rule 5 requires every feature subset to compile, and the rule was
# written down without anything enforcing it: a virtual component that reaches for
# a part its own feature list does not name still compiles as long as every board
# feature happens to enable both. That is how `display-light` was found importing
# the backlight while not depending on it — a class of defect the documented
# builds cannot see, because a board feature is by construction the union of the
# things a board wires.
#
# So each feature is checked alone rather than in combination. The split follows
# what the crate can build: the four pure `embedded-hal` components check on the
# host, and everything that reaches for `esp-hal` checks against the bare-metal
# target through iot-xtensa.sh. `cargo hack --feature-powerset` is the general
# answer but is not in the devshell; this is the subset that actually matters here,
# and it needs no new dependency.
#
# Board features are not listed: each is built by check-c6 / check-s3 already.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BSP="iot-bsp-esp"
TARGET="xtensa-esp32s3-none-elf"

# Pure `embedded-hal` drivers: no chip HAL, so the host builds them.
HOST_COMPONENTS=(pca9557 qmi8658 es7210 es8311 ft6336)

# Everything that needs `esp-hal`, and therefore the bare-metal target. The chip
# feature is what the board features normally contribute; named here so each
# subset can stand on its own.
CHIP_COMPONENTS=(
  "backlight"
  "button"
  "ws2812"
  "st7789"
  "display-light"
  "audio"
  "audio-out"
)

failed=()

for component in "${HOST_COMPONENTS[@]}"; do
  if (cd "$ROOT/apps/iot" && cargo check -p "$BSP" --no-default-features --features "$component") >/dev/null 2>&1; then
    echo "  ok   $component (host)"
  else
    echo "  FAIL $component (host)"
    failed+=("$component")
  fi
done

for component in "${CHIP_COMPONENTS[@]}"; do
  if (cd "$ROOT/apps/iot" && bash "$ROOT/scripts/iot-xtensa.sh" check \
        -p "$BSP" --no-default-features --features "$component,esp-hal/esp32s3" \
        --target "$TARGET" -Z build-std=core,alloc) >/dev/null 2>&1; then
    echo "  ok   $component (bare-metal)"
  else
    echo "  FAIL $component (bare-metal)"
    failed+=("$component")
  fi
done

if [ ${#failed[@]} -gt 0 ]; then
  echo >&2
  echo "these features do not build alone: ${failed[*]}" >&2
  echo "each must name every part it reads, so a board feature is an aggregation" >&2
  echo "of what that board needs rather than what the component happened to rely on" >&2
  exit 1
fi