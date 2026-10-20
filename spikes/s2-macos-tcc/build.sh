#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Builds the four test apps for spike S2:
#   adhoc      0.0.1 + 0.0.2   ad-hoc signature (what Tauri does by default without an identity)
#   selfsigned 0.0.1 + 0.0.2   signed with the stable self-signed certificate from make-cert.sh
#
# Output (all in out/):
#   dmg/<mode>-<version>.dmg   "downloaded" installers (quarantine flag set, like a browser download)
#   update/<mode>-<version>/   the same .app WITHOUT quarantine, as the in-app updater would install it
set -euo pipefail
cd "$(dirname "$0")"
# Tauri's bundler calls `xattr -crs`; Python/pyenv shims named xattr don't support it.
# Put Apple's system tools first.
export PATH="/usr/bin:/bin:/usr/sbin:/sbin:$PATH"

MODES="adhoc selfsigned"
VERSIONS="0.0.1 0.0.2"
KC="$PWD/out/signing/kipple-spike.keychain-db"

[ -d node_modules ] || pnpm install --silent
if [ ! -f src-tauri/icons/icon.icns ]; then
  mkdir -p out/icon
  qlmanage -t -s 1024 -o out/icon hen-icon.svg >/dev/null
  pnpm -s tauri icon out/icon/hen-icon.svg.png -o src-tauri/icons >/dev/null
fi
./make-cert.sh
IDENTITY=$(cat out/signing/identity.txt)
security unlock-keychain -p kipple-spike-throwaway "$KC"

for mode in $MODES; do
  for ver in $VERSIONS; do
    name="Kipple TCC Spike ($mode)"
    id="app.kipple.spike.tcc.$mode"
    echo "==> $mode $ver"
    pnpm -s tauri build --bundles app \
      --config "{\"version\":\"$ver\",\"identifier\":\"$id\",\"productName\":\"$name\"}" >/dev/null
    built="src-tauri/target/release/bundle/macos/$name.app"

    dest="out/update/$mode-$ver"
    rm -rf "$dest" && mkdir -p "$dest"
    ditto "$built" "$dest/$name.app"

    if [ "$mode" = selfsigned ]; then
      codesign --force --sign "$IDENTITY" --keychain "$KC" --identifier "$id" \
        --timestamp=none "$dest/$name.app"
    fi
    codesign --verify --strict "$dest/$name.app"

    mkdir -p out/dmg
    hdiutil create -quiet -ov -fs HFS+ -format UDZO -volname "$name $ver" \
      -srcfolder "$dest/$name.app" "out/dmg/$mode-$ver.dmg"
    # Simulate a browser download so Gatekeeper treats it like a real one.
    xattr -w com.apple.quarantine "0081;$(printf '%x' "$(date +%s)");Safari;" "out/dmg/$mode-$ver.dmg"
  done
done

echo
echo "Built: out/dmg/*.dmg (quarantined) and out/update/* (updater-style copies)."
echo "Next: ./inspect.sh, then follow the checklist in README.md."
