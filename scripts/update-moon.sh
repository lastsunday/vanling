#!/usr/bin/env bash
set -euo pipefail

FLAKE="$(cd "$(dirname "$0")/.." && pwd)/flake.nix"

CHECK=false
if [ "${1:-}" = "--check" ]; then
  CHECK=true
  shift
fi

LATEST="${1:-}"
if [ -z "$LATEST" ]; then
  LATEST=$(curl -sSfL -o /dev/null -w '%{url_effective}' \
    "https://github.com/moonrepo/moon/releases/latest" | sed 's|.*/v||')
fi

CURRENT=$(grep 'moonVersion = "' "$FLAKE" | sed 's/.*"\(.*\)".*/\1/')

if [ "$LATEST" = "$CURRENT" ]; then
  echo "moon is already at $LATEST"
  exit 0
fi

if $CHECK; then
  echo "moon $CURRENT -> $LATEST available (run without --check to update)"
  exit 0
fi

echo "Updating moon: $CURRENT -> $LATEST"

TARGETS=(
  "x86_64-linux:x86_64-unknown-linux-gnu"
  "aarch64-linux:aarch64-unknown-linux-gnu"
  "x86_64-darwin:x86_64-apple-darwin"
  "aarch64-darwin:aarch64-apple-darwin"
)

for pair in "${TARGETS[@]}"; do
  nix_name="${pair%%:*}"
  gh_name="${pair##*:}"
  url="https://github.com/moonrepo/moon/releases/download/v${LATEST}/moon_cli-${gh_name}.tar.xz.sha256"
  hash=$(curl -sSfL "$url" | awk '{print $1}')
  sed -i '' "/${nix_name}/s/\"[a-f0-9]\{64\}\"/\"${hash}\"/" "$FLAKE"
  echo "  $nix_name: $hash"
done

sed -i '' "s/moonVersion = \"${CURRENT}\"/moonVersion = \"${LATEST}\"/" "$FLAKE"

echo "Done. Run 'nix build .#moon' to verify."
