#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Replaces /Applications/Kipple TCC Spike (<mode>).app with version 0.0.2, the way an in-app
# updater does: swap the bundle in place, without a quarantine flag.
#   ./simulate-update.sh adhoc | selfsigned
set -euo pipefail
cd "$(dirname "$0")"

mode="${1:?usage: ./simulate-update.sh adhoc|selfsigned}"
case "$mode" in adhoc|selfsigned) ;; *) echo "mode must be adhoc or selfsigned" >&2; exit 2;; esac
name="Kipple TCC Spike ($mode)"
src="out/update/$mode-0.0.2/$name.app"

# Wherever the user installed it: /Applications or ~/Applications.
target=""
for dir in "/Applications" "$HOME/Applications"; do
  [ -d "$dir/$name.app" ] && target="$dir/$name.app" && break
done

[ -d "$src" ] || { echo "Missing $src. Run ./build.sh first." >&2; exit 1; }
[ -n "$target" ] || { echo "Install 0.0.1 first: '$name.app' not found in /Applications or ~/Applications." >&2; exit 1; }
osascript -e "tell application \"$name\" to quit" 2>/dev/null || true
sleep 1
rm -rf "$target"
ditto "$src" "$target"
echo "Updated $target to 0.0.2 (no quarantine flag, like the in-app updater)."
