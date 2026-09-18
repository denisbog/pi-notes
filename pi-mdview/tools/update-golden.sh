#!/usr/bin/env bash
# Regenerates the golden files in tools/golden/ from pi's own renderer.
#
#   PI_TUI_PATH=... pi-mdview/tools/update-golden.sh 40 60 80 100
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fixture="$here/parity.md"
mkdir -p "$here/golden"

widths=("$@")
if [ ${#widths[@]} -eq 0 ]; then
  widths=(40 60 80 100)
fi

for width in "${widths[@]}"; do
  node "$here/pi_render.mjs" "$width" "$fixture" \
    | python3 -c 'import json,sys; print("\n".join(json.load(sys.stdin)), end="\n")' \
    > "$here/golden/pi-$width.txt"
  echo "wrote golden/pi-$width.txt"
done
