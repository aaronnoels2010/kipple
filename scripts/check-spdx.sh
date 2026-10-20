#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Checks that source files carry the project licence header within their first 5 lines.
#
#   scripts/check-spdx.sh            # check all files tracked by git
#   scripts/check-spdx.sh FILE...    # check the given files
#
# Only source files are checked (see SOURCE_EXT). Generated and vendored paths are skipped.
set -u

HEADER="SPDX-License-Identifier: GPL-3.0-or-later"
SOURCE_EXT='\.(rs|ts|tsx|js|jsx|mjs|cjs|css|scss|sh)$'
SKIP_PATH='(^|/)(node_modules|target|dist|spikes|app/src/bindings|docs)/'

if [ "$#" -gt 0 ]; then
  files=$(printf '%s\n' "$@")
else
  files=$(git ls-files)
fi

missing=0
checked=0
while IFS= read -r f; do
  [ -n "$f" ] || continue
  printf '%s\n' "$f" | grep -Eq "$SOURCE_EXT" || continue
  if [ "$#" -eq 0 ] && printf '%s\n' "$f" | grep -Eq "$SKIP_PATH"; then continue; fi
  checked=$((checked + 1))
  if ! head -n 5 "$f" | grep -qF "$HEADER"; then
    echo "missing licence header: $f" >&2
    missing=$((missing + 1))
  fi
done <<EOF
$files
EOF

if [ "$missing" -gt 0 ]; then
  echo "$missing of $checked source file(s) lack '$HEADER' in their first 5 lines." >&2
  exit 1
fi
echo "SPDX headers OK ($checked source file(s) checked)."
