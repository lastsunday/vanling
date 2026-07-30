#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<EOF
Usage: $0 --prefix <prefix> [--keep <N>] [--dry-run]

Clean up old GitHub releases and tags matching a given prefix.

Options:
  --prefix PREFIX   Tag prefix (e.g. dev-server-, dev-app-)
  --keep N          Number of latest releases to keep (default: 5)
  --dry-run         Only list what would be deleted, don't delete
  -h, --help        Show this help
EOF
  exit 1
}

KEEP=5
DRY_RUN=false
PREFIX=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix) PREFIX="$2"; shift 2 ;;
    --keep)   KEEP="$2"; shift 2 ;;
    --dry-run) DRY_RUN=true; shift ;;
    -h|--help) usage ;;
    *) echo "Unknown option: $1"; usage ;;
  esac
done

if [ -z "$PREFIX" ]; then
  echo "Error: --prefix is required"
  usage
fi

if ! command -v gh &>/dev/null; then
  echo "Error: gh CLI not found"
  exit 1
fi

if ! gh auth status &>/dev/null; then
  echo "Error: not authenticated with gh"
  exit 1
fi

echo "Fetching all tags matching '${PREFIX}'..."

tags=$(gh api --paginate "/repos/${GITHUB_REPOSITORY:-$(gh repo view --json nameWithOwner -q .nameWithOwner)}/git/refs/tags" \
  --jq '.[].ref' | grep "^refs/tags/${PREFIX}" | sort -V || true)

tag_count=$(echo "$tags" | grep -c . || true)
echo "Found ${tag_count} tags"

if [ "$tag_count" -le "$KEEP" ]; then
  echo "All tags within retention limit (keep=${KEEP}), nothing to clean"
  exit 0
fi

tags_to_delete=$(echo "$tags" | head -n -"${KEEP}")

echo "Tags/releases to delete:"
echo "$tags_to_delete" | while read ref; do
  tag="${ref#refs/tags/}"
  echo "  $tag"
done

if [ "$DRY_RUN" = true ]; then
  echo "Dry-run mode, skipping actual deletion"
  exit 0
fi

echo "$tags_to_delete" | while read ref; do
  tag="${ref#refs/tags/}"
  echo "Deleting release: $tag"
  gh release delete "$tag" --yes || echo "  (release not found, skipping)"
  echo "Deleting tag: $tag"
  gh api -X DELETE "/repos/${GITHUB_REPOSITORY:-$(gh repo view --json nameWithOwner -q .nameWithOwner)}/git/refs/${ref#refs/}" || echo "  (tag not found, skipping)"
done

echo "Cleanup complete"
