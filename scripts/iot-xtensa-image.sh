#!/usr/bin/env bash
set -euo pipefail

# Builds the Xtensa image `scripts/iot-xtensa.sh` falls back to when the host has
# no native espup toolchain. Idempotent: an existing tag is reused, so the
# 10-20 min build runs once per toolchain version. Pass `--force` to rebuild.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IMAGE="$(bash "$SCRIPT_DIR/iot-xtensa.sh" --image)"
XTENSA_VERSION="${IMAGE##*_}"

if [ "${1:-}" != "--force" ] && docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "[iot-xtensa-image] present: $IMAGE" >&2
  exit 0
fi

echo "[iot-xtensa-image] building $IMAGE (Xtensa $XTENSA_VERSION)" >&2
docker build \
  --file "$SCRIPT_DIR/iot-xtensa.Containerfile" \
  --build-arg "XTENSA_VERSION=$XTENSA_VERSION" \
  --tag "$IMAGE" \
  "$SCRIPT_DIR"
