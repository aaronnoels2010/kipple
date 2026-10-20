#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Runs spike S1 against: a folder in $HOME (→ ~/.Trash) and four throwaway disk images that act
# as external drives (APFS, Mac OS Extended, ExFAT, FAT32 → <volume>/.Trashes/<uid>).
# All test files are created in a kipple-s1-<pid> subfolder and removed (or restored) afterwards.
# Output: out/report.md
set -euo pipefail
cd "$(dirname "$0")"
export PATH="/usr/bin:/bin:/usr/sbin:/sbin:$PATH"

cargo build --release --quiet
BIN="target/release/kipple-spike-trash"
mkdir -p out/img out/mnt

declare -a mounts=()
cleanup() { for m in "${mounts[@]:-}"; do [ -n "$m" ] && hdiutil detach -quiet -force "$m" 2>/dev/null || true; done; }
trap cleanup EXIT

for spec in "APFS:APFS" "HFS+J:HFSJ" "ExFAT:EXFAT" "MS-DOS FAT32:FAT32"; do
  fs="${spec%%:*}"; tag="${spec##*:}"
  img="out/img/$tag.dmg"; mnt="$PWD/out/mnt/$tag"
  rm -f "$img"; mkdir -p "$mnt"
  hdiutil create -quiet -size 64m -fs "$fs" -volname "KS1$tag" -ov "$img"
  hdiutil attach -quiet -nobrowse -mountpoint "$mnt" "$img"
  mounts+=("$mnt")
done

{
  echo "# Spike S1 report"
  echo
  echo "macOS $(sw_vers -productVersion) ($(uname -m)) · $(date '+%Y-%m-%d %H:%M')"
  echo
  "$BIN" "$HOME" "${mounts[@]}"
} | tee out/report.md
