#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Spike S9 on macOS: home folder + throwaway disk images (APFS, ExFAT, FAT32, read-only APFS).
# Each probe trashes one throwaway file and takes it out of the Trash again.
set -euo pipefail
cd "$(dirname "$0")"
export PATH="/usr/bin:/bin:/usr/sbin:/sbin:$PATH"

cargo build --release --quiet
mkdir -p out/img out/mnt
mounts=()
cleanup() { for m in "${mounts[@]:-}"; do [ -n "$m" ] && hdiutil detach -quiet -force "$m" 2>/dev/null || true; done; }
trap cleanup EXIT

attach() { # attach <fs> <tag> [readonly]
  local img="out/img/$2.dmg" mnt="$PWD/out/mnt/$2"
  rm -f "$img"; mkdir -p "$mnt"
  # 64 MB: FAT32 needs more than 32 MB
  hdiutil create -quiet -size 64m -fs "$1" -volname "KS9$2" -ov "$img"
  hdiutil attach -quiet -nobrowse ${3:+-readonly} -mountpoint "$mnt" "$img"
  mounts+=("$mnt")
}
attach APFS APFS
attach ExFAT EXFAT
attach "MS-DOS FAT32" FAT32
attach APFS APFS_RO readonly

mkdir -p out
{
  echo "## macOS $(sw_vers -productVersion) · $(uname -m) · $(date '+%Y-%m-%d %H:%M')"
  echo
  target/release/kipple-spike-trashprobe \
    "home=$HOME" "apfs-image=$PWD/out/mnt/APFS" "exfat-image=$PWD/out/mnt/EXFAT" \
    "fat32-image=$PWD/out/mnt/FAT32" "read-only-image=$PWD/out/mnt/APFS_RO" || true
} | tee out/results-macos.md
