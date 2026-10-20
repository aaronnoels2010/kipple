#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Compares the code-signing "designated requirement" (DR) of v0.0.1 and v0.0.2 per mode.
# TCC remembers a permission for the app's DR: if the DR changes between versions,
# macOS treats the update as a different app and asks for folder access again.
set -euo pipefail
cd "$(dirname "$0")"

report="out/inspect.txt"
: > "$report"

# Ad-hoc apps have no explicit DR; codesign prints the implicit one as "# designated => cdhash H...".
dr_of() { codesign -d -r- "$1" 2>&1 | sed -n 's/^#* *designated => //p'; }

for mode in adhoc selfsigned; do
  name="Kipple TCC Spike ($mode)"
  {
    echo "## $mode"
    for ver in 0.0.1 0.0.2; do
      app="out/update/$mode-$ver/$name.app"
      echo "### $ver"
      codesign -dvvv "$app" 2>&1 | grep -E '^(Identifier|Signature|Authority|TeamIdentifier|CDHash)=' || true
      echo "DR: $(dr_of "$app")"
    done
    dr1=$(dr_of "out/update/$mode-0.0.1/$name.app")
    dr2=$(dr_of "out/update/$mode-0.0.2/$name.app")
    if [ -z "$dr1" ] || [ -z "$dr2" ]; then
      echo "RESULT: UNKNOWN (could not read a designated requirement)"
    elif [ "$dr1" = "$dr2" ]; then
      echo "RESULT: DR is STABLE across versions -> TCC grant expected to survive the update"
    else
      echo "RESULT: DR CHANGES across versions -> TCC expected to ask again after the update"
    fi
    echo
  } | tee -a "$report"
done
echo "Saved to $report"
