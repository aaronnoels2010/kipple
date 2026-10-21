// SPDX-License-Identifier: GPL-3.0-or-later
//! The adapters for the OS Kipple runs on, picked at compile time.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::NativeVolumes;
#[cfg(target_os = "macos")]
pub use macos::NativeVolumes;
#[cfg(windows)]
pub use windows::NativeVolumes;

/// Shared by the adapters: filesystems whose file IDs survive renames and remounts.
pub(crate) fn has_stable_file_ids(fs_type: &str) -> bool {
    matches!(
        fs_type.to_ascii_lowercase().as_str(),
        "apfs"
            | "hfs"
            | "ntfs"
            | "refs"
            | "ext2"
            | "ext3"
            | "ext4"
            | "btrfs"
            | "xfs"
            | "zfs"
            | "f2fs"
            | "bcachefs"
            | "tmpfs"
            | "jfs"
            | "reiserfs"
    )
}

/// Shared by the Unix adapters: network filesystems (never a Trash Kipple can rely on, S9).
#[cfg(unix)]
pub(crate) fn is_network_fs(fs_type: &str) -> bool {
    let fs = fs_type.to_ascii_lowercase();
    matches!(
        fs.as_str(),
        "nfs"
            | "nfs4"
            | "cifs"
            | "smb"
            | "smb2"
            | "smb3"
            | "smbfs"
            | "afpfs"
            | "webdav"
            | "davfs"
            | "9p"
            | "ceph"
            | "glusterfs"
            | "fuse.sshfs"
            | "fuse.rclone"
            | "fuse.s3fs"
    ) || fs.starts_with("fuse.") && fs != "fuse.portal"
}
