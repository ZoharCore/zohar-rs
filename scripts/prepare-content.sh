#!/usr/bin/env bash
set -euo pipefail

src="data/content"
dst=".local/content"

if [[ ! -d "$src" ]]; then
  echo "content source directory not found: $src" >&2
  exit 1
fi

mkdir -p "$dst"
rsync -a --delete "$src"/ "$dst"/

echo "staged content into $dst from $src"
