#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Spike S9 on Linux (CI runner, needs sudo): home + loop-mounted ext4 / vfat / exfat images,
# tmpfs, and real NFS and SMB shares served from localhost.
set -euo pipefail
cd "$(dirname "$0")"

sudo apt-get update -qq
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq exfatprogs dosfstools nfs-kernel-server samba cifs-utils >/dev/null
U=$(id -u); G=$(id -g); ME=$(whoami)
cargo build --release --quiet
mkdir -p out

loop() { # loop <tag> <mkfs command…>
  local tag="$1"; shift
  truncate -s 64M "out/$tag.img"
  "$@" "out/$tag.img" >/dev/null
  sudo mkdir -p "/mnt/k-$tag"
}
loop ext4 mkfs.ext4 -q -F
sudo mount -o loop "out/ext4.img" /mnt/k-ext4 && sudo chown "$U:$G" /mnt/k-ext4
loop vfat mkfs.vfat
sudo mount -o "loop,uid=$U,gid=$G" "out/vfat.img" /mnt/k-vfat
loop exfat mkfs.exfat
sudo mount -o "loop,uid=$U,gid=$G" "out/exfat.img" /mnt/k-exfat
sudo mkdir -p /mnt/k-tmpfs && sudo mount -t tmpfs -o "size=16M,uid=$U,gid=$G" tmpfs /mnt/k-tmpfs

# NFS (v4) from localhost
sudo mkdir -p /srv/k-nfs /mnt/k-nfs && sudo chown "$U:$G" /srv/k-nfs
echo "/srv/k-nfs 127.0.0.1(rw,sync,no_subtree_check,all_squash,anonuid=$U,anongid=$G,insecure)" | sudo tee /etc/exports >/dev/null
sudo exportfs -ra && sudo systemctl restart nfs-kernel-server
sudo mount -t nfs -o vers=4 127.0.0.1:/srv/k-nfs /mnt/k-nfs

# SMB (Samba guest share) from localhost
sudo mkdir -p /srv/k-smb /mnt/k-smb && sudo chown "$U:$G" /srv/k-smb
sudo tee -a /etc/samba/smb.conf >/dev/null <<EOF
[kshare]
   path = /srv/k-smb
   read only = no
   guest ok = yes
   force user = $ME
EOF
sudo systemctl restart smbd
sudo mount -t cifs //127.0.0.1/kshare /mnt/k-smb -o "guest,uid=$U,gid=$G,vers=3.0"

{
  echo "## Linux $(. /etc/os-release && echo "$PRETTY_NAME") · $(uname -m) · $(date '+%Y-%m-%d %H:%M')"
  echo
  target/release/kipple-spike-trashprobe \
    "home=$HOME" "tmp=/tmp" "ext4-loop=/mnt/k-ext4" "vfat-loop=/mnt/k-vfat" "exfat-loop=/mnt/k-exfat" \
    "tmpfs=/mnt/k-tmpfs" "nfs4-localhost=/mnt/k-nfs" "smb3-localhost=/mnt/k-smb" || true
} | tee out/results-linux.md
