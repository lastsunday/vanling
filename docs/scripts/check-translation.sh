#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$script_dir/.." && pwd)"
content_dir="$root/content"
stale=0

extract_frontmatter() {
  sed -n '/^+++/,/^+++/p' "$1"
}

get_source_file_hash() {
  extract_frontmatter "$1" | sed -n '/^\[extra\]/,/^+++/p' | grep '^source_file_hash' | sed 's/^source_file_hash = "\(.*\)"$/\1/' | head -1 || true
}

while IFS= read -r -d '' en_file; do
  en_rel="${en_file#$content_dir/}"
  source_file="${en_file%.en.md}.md"

  if [ ! -f "$source_file" ]; then
    echo "⚠️  NO_SOURCE: $en_rel (missing $source_file)"
    stale=1
    continue
  fi

  current_hash=$(git hash-object "$source_file" 2>/dev/null || echo "")
  stored_hash=$(get_source_file_hash "$en_file" || true)

  if [ -z "$stored_hash" ]; then
    echo "⚠️  UNTRANSLATED: $en_rel"
    stale=1
  elif [ "$current_hash" != "$stored_hash" ]; then
    echo "⚠️  STALE: $en_rel (source changed: $stored_hash → $current_hash)"
    stale=1
  fi
done < <(find "$content_dir" -name '*.en.md' -print0)

exit $stale
