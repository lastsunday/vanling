#!/usr/bin/env bash
set -euo pipefail

FLAKE="$(cd "$(dirname "$0")/.." && pwd)/flake.nix"

VERSION=$(grep 'sherpaOnnx = "' "$FLAKE" | sed 's/.*"\(.*\)".*/\1/')
BASE_URL="https://github.com/k2-fsa/sherpa-onnx/releases/download/v${VERSION}"

echo "sherpa-onnx version: $VERSION"

declare -A PLATFORMS=(
  [x86_64-linux]="linux-x64"
  [aarch64-linux]="linux-aarch64"
  [x86_64-darwin]="osx-x64"
  [aarch64-darwin]="osx-arm64"
)

for nix_sys in x86_64-linux aarch64-linux x86_64-darwin aarch64-darwin; do
  arch="${PLATFORMS[$nix_sys]}"
  archive="sherpa-onnx-v${VERSION}-${arch}-static-lib.tar.bz2"
  url="${BASE_URL}/${archive}"

  echo -n "  $nix_sys: prefetching $archive ... "

  base32=$(nix-prefetch-url --unpack --type sha256 "$url" 2>/dev/null | tail -1)
  sri_hash=$(nix hash to-sri --type sha256 "$base32")

  echo "$sri_hash"

  tmp_file=$(mktemp)
  sed "/${nix_sys}/,/sherpaOnnxHash/s#sherpaOnnxHash = \"sha256-[^\"]*\"#sherpaOnnxHash = \"${sri_hash}\"#" "$FLAKE" > "$tmp_file"
  mv "$tmp_file" "$FLAKE"
done

# sherpaOnnxLibArm64 uses the same aarch64-linux hash
arm64_hash=$(grep 'sherpaOnnxHash = "sha256-' "$FLAKE" | head -1 | sed 's/.*"\(sha256-[^"]*\)".*/\1/')
tmp_file=$(mktemp)
sed "/sherpaOnnxLibArm64/,/stripRoot/s#hash = \"sha256-[^\"]*\"#hash = \"${arm64_hash}\"#" "$FLAKE" > "$tmp_file"
mv "$tmp_file" "$FLAKE"

echo ""
echo "Done. Run 'nix flake check' to verify."
