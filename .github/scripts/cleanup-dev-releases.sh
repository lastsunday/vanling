#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<EOF
Usage: $0 --prefix <prefix> [--keep <N>] [--dry-run]

Clean up old GitHub releases (including drafts) and tags matching a given prefix.

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

REPO="${GITHUB_REPOSITORY:-$(gh repo view --json nameWithOwner -q .nameWithOwner)}"

echo "Fetching all releases matching '${PREFIX}'..."

releases_json=$(gh api --paginate \
  "/repos/${REPO}/releases" \
  --jq '.[] | {id, tag_name, draft, created_at}' 2>/dev/null \
  | jq -s --arg p "$PREFIX" \
    '[.[] | select(.tag_name | startswith($p))] | sort_by(.created_at) | reverse'
) || echo "[]"

total=$(echo "$releases_json" | jq 'length' 2>/dev/null || echo 0)
echo "Found ${total} releases matching ${PREFIX}"

if [ "$total" -le "$KEEP" ]; then
  echo "All releases within retention limit (keep=${KEEP})"
  exit 0
fi

echo "Releases to delete ($((total - KEEP)) oldest):"
echo "$releases_json" | jq -r '.['"${KEEP}"':][] | "  \(.tag_name) (draft=\(.draft))"'

if [ "$DRY_RUN" = true ]; then
  echo "Dry-run mode, skipping actual deletion"
  exit 0
fi

echo "$releases_json" | jq -r '.['"${KEEP}"':][] | "\(.id) \(.tag_name) \(.draft)"' | \
while read id tag_name is_draft; do
  echo "Deleting: ${tag_name} (id=${id})"
  if [ "$is_draft" = "true" ]; then
    gh api -X DELETE "/repos/${REPO}/releases/${id}" || echo "  (release delete failed)"
  else
    gh release delete "$tag_name" --yes 2>/dev/null || \
      gh api -X DELETE "/repos/${REPO}/releases/${id}" || echo "  (release delete failed)"
  fi
  gh api -X DELETE "/repos/${REPO}/git/refs/tags/${tag_name}" 2>/dev/null || echo "  (tag not found)"
done

echo "Cleanup complete"
