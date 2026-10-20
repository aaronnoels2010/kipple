#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Tests for scripts/check-spdx.sh. Run: bash scripts/tests/check-spdx.test.sh
set -u

HERE="$(cd "$(dirname "$0")" && pwd)"
CHECK="$HERE/../check-spdx.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

pass=0
fail=0

expect() { # expect <description> <expected-exit-code> <files...>
  local desc="$1" want="$2"; shift 2
  local out got
  out="$(bash "$CHECK" "$@" 2>&1)"; got=$?
  if [ "$got" -eq "$want" ]; then
    pass=$((pass + 1)); echo "ok   - $desc"
  else
    fail=$((fail + 1)); echo "FAIL - $desc (exit $got, want $want)"; echo "$out" | sed 's/^/       /'
  fi
}

expect_output() { # expect_output <description> <needle> <files...>
  local desc="$1" needle="$2"; shift 2
  if bash "$CHECK" "$@" 2>&1 | grep -qF "$needle"; then
    pass=$((pass + 1)); echo "ok   - $desc"
  else
    fail=$((fail + 1)); echo "FAIL - $desc (output lacks: $needle)"
  fi
}

printf '// SPDX-License-Identifier: GPL-3.0-or-later\nfn main() {}\n' > "$TMP/good.rs"
printf '/* SPDX-License-Identifier: GPL-3.0-or-later */\n:root {}\n' > "$TMP/good.css"
printf '#!/usr/bin/env bash\n# SPDX-License-Identifier: GPL-3.0-or-later\necho hi\n' > "$TMP/good.sh"
printf 'fn main() {}\n' > "$TMP/missing.rs"
printf '// SPDX-License-Identifier: MIT\nexport {}\n' > "$TMP/wrong.ts"
printf 'a\nb\nc\nd\ne\n// SPDX-License-Identifier: GPL-3.0-or-later\n' > "$TMP/too-late.tsx"
printf '# Readme without header\n' > "$TMP/notes.md"

expect "accepts files with the header (rs, css, sh)" 0 "$TMP/good.rs" "$TMP/good.css" "$TMP/good.sh"
expect "rejects a source file without a header" 1 "$TMP/missing.rs"
expect "rejects a different licence identifier" 1 "$TMP/wrong.ts"
expect "rejects a header after the first 5 lines" 1 "$TMP/too-late.tsx"
expect "ignores non-source files (md)" 0 "$TMP/notes.md"
expect "fails when any one file is bad" 1 "$TMP/good.rs" "$TMP/missing.rs"
expect_output "names the offending file" "missing.rs" "$TMP/good.rs" "$TMP/missing.rs"

echo "# $pass passed, $fail failed"
[ "$fail" -eq 0 ]
