// SPDX-License-Identifier: GPL-3.0-or-later
//! The adapters for the OS Kipple runs on, picked at compile time.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
mod unix;

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod screenshot_names;

#[cfg(target_os = "linux")]
pub use linux::{
    NativeDownloadOrigins, NativePlaceholders, NativeScreenshots, NativeSyncRoots, NativeVolumes,
};
#[cfg(target_os = "macos")]
pub use macos::{
    NativeDownloadOrigins, NativePlaceholders, NativeScreenshots, NativeSyncRoots, NativeVolumes,
};
#[cfg(unix)]
pub use unix::NativeDirectories;
#[cfg(windows)]
pub use windows::{
    NativeDirectories, NativeDownloadOrigins, NativePlaceholders, NativeScreenshots,
    NativeSyncRoots, NativeVolumes,
};

/// The most memory this process has had resident at once, in bytes: for benchmarks.
pub fn peak_memory() -> Option<u64> {
    #[cfg(unix)]
    {
        // SAFETY: getrusage only writes into the zeroed struct it's given.
        let usage = unsafe {
            let mut usage: libc::rusage = std::mem::zeroed();
            (libc::getrusage(libc::RUSAGE_SELF, &mut usage) == 0).then_some(usage)
        }?;
        let max = u64::try_from(usage.ru_maxrss).ok()?;
        // Bytes on macOS, KiB on Linux.
        Some(if cfg!(target_os = "macos") {
            max
        } else {
            max * 1024
        })
    }
    #[cfg(windows)]
    {
        windows::peak_memory()
    }
}

/// Sets this process's cloud-file behaviour; call once at startup. macOS: reading a dataless
/// file fails instead of downloading it. Windows: cloud files are shown as what they are, but
/// reading one still downloads it, so readers check `is_placeholder` first. Linux has none.
pub fn never_download_placeholders() -> crate::PortResult<()> {
    #[cfg(target_os = "macos")]
    return macos::never_download_placeholders();
    #[cfg(windows)]
    return windows::never_download_placeholders();
    #[cfg(target_os = "linux")]
    Ok(())
}

use crate::SyncProvider;

/// `path` with symlinks (and junctions) resolved, in the form the rest of Kipple compares: on
/// Windows without the `\\?\` prefix `canonicalize` adds.
pub fn canonical(path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    #[cfg(windows)]
    return windows::resolve(path);
    #[cfg(not(windows))]
    std::fs::canonicalize(path)
}

/// The provider of a macOS File Provider folder in `~/Library/CloudStorage`, by its name
/// (`OneDrive-Personal`, `Dropbox`, `GoogleDrive-zoe@example.com`, `Box-Box`…).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn provider_of_cloud_storage(folder: &str) -> SyncProvider {
    let brand = folder.split('-').next().unwrap_or(folder);
    match brand.to_ascii_lowercase().as_str() {
        "onedrive" => SyncProvider::OneDrive,
        "dropbox" => SyncProvider::Dropbox,
        "googledrive" => SyncProvider::GoogleDrive,
        "icloud" | "iclouddrive" => SyncProvider::ICloudDrive,
        _ => SyncProvider::Other(folder.to_string()),
    }
}

/// The provider of a Windows sync root, by its SyncRootManager key
/// (`OneDrive!S-1-5-21-…!Personal`, `Dropbox!S-1-…!1`, `iCloudDrive!…`, `GoogleDriveFS!…`).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn provider_of_sync_root_key(key: &str) -> SyncProvider {
    let brand = key.split('!').next().unwrap_or(key);
    match brand.to_ascii_lowercase().as_str() {
        "onedrive" => SyncProvider::OneDrive,
        "dropbox" => SyncProvider::Dropbox,
        "iclouddrive" | "icloud" => SyncProvider::ICloudDrive,
        b if b.starts_with("googledrive") => SyncProvider::GoogleDrive,
        _ => SyncProvider::Other(brand.to_string()),
    }
}

/// The download URL in a Windows `Zone.Identifier` stream: `HostUrl` in `[ZoneTransfer]`.
/// `ReferrerUrl` (the page that linked to the file) is no stand-in, so every OS answers with
/// the file's own URL; browsers write `about:internet` when they keep the URL to themselves.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn zone_host_url(stream: &[u8]) -> Option<String> {
    // Usually ANSI or UTF-8; some tools write UTF-16 with a byte order mark.
    let text = match stream {
        [0xFF, 0xFE, utf16 @ ..] => String::from_utf16_lossy(
            &utf16
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&pair| u16::from_le_bytes(pair))
                .collect::<Vec<_>>(),
        ),
        [0xEF, 0xBB, 0xBF, utf8 @ ..] | utf8 => String::from_utf8_lossy(utf8).into_owned(),
    };
    let mut in_zone_transfer = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_zone_transfer = line.eq_ignore_ascii_case("[ZoneTransfer]");
        } else if in_zone_transfer
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("HostUrl")
        {
            let url = value.trim();
            return (!url.is_empty() && !url.eq_ignore_ascii_case("about:internet"))
                .then(|| url.to_string());
        }
    }
    None
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_by_cloud_storage_folder() {
        assert_eq!(
            provider_of_cloud_storage("OneDrive-Personal"),
            SyncProvider::OneDrive
        );
        assert_eq!(
            provider_of_cloud_storage("OneDrive-Contoso"),
            SyncProvider::OneDrive
        );
        assert_eq!(provider_of_cloud_storage("Dropbox"), SyncProvider::Dropbox);
        assert_eq!(
            provider_of_cloud_storage("GoogleDrive-zoe@example.com"),
            SyncProvider::GoogleDrive
        );
        assert_eq!(
            provider_of_cloud_storage("Box-Box"),
            SyncProvider::Other("Box-Box".into())
        );
    }

    #[test]
    fn the_download_url_is_the_host_url_of_the_zone_transfer() {
        let stream = b"[ZoneTransfer]\r\nZoneId=3\r\nReferrerUrl=https://example.com/downloads\r\nHostUrl=https://example.com/files/report.pdf\r\n";
        assert_eq!(
            zone_host_url(stream).as_deref(),
            Some("https://example.com/files/report.pdf")
        );
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain(
                "[ZoneTransfer]\r\nhosturl = https://example.com/a.zip\r\n"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes),
            )
            .collect();
        assert_eq!(
            zone_host_url(&utf16).as_deref(),
            Some("https://example.com/a.zip"),
            "UTF-16, any case, spaces around the ="
        );
        assert_eq!(
            zone_host_url(b"\xEF\xBB\xBF[ZoneTransfer]\nHostUrl=https://example.com/b\n")
                .as_deref(),
            Some("https://example.com/b")
        );
    }

    #[test]
    fn zone_transfers_without_a_host_url_have_no_origin() {
        for stream in [
            &b"[ZoneTransfer]\r\nZoneId=3\r\n"[..],
            b"[ZoneTransfer]\r\nZoneId=3\r\nReferrerUrl=https://example.com/\r\n",
            b"[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=about:internet\r\n",
            b"[ZoneTransfer]\r\nHostUrl=\r\n",
            b"[Other]\r\nHostUrl=https://example.com/a.zip\r\n",
            b"",
        ] {
            assert_eq!(
                zone_host_url(stream),
                None,
                "{}",
                String::from_utf8_lossy(stream)
            );
        }
    }

    #[test]
    fn providers_by_sync_root_key() {
        assert_eq!(
            provider_of_sync_root_key("OneDrive!S-1-5-21-1!Personal"),
            SyncProvider::OneDrive
        );
        assert_eq!(
            provider_of_sync_root_key("Dropbox!S-1-5-21-1!1"),
            SyncProvider::Dropbox
        );
        assert_eq!(
            provider_of_sync_root_key("iCloudDrive!S-1-5-21-1!x"),
            SyncProvider::ICloudDrive
        );
        assert_eq!(
            provider_of_sync_root_key("GoogleDriveFS!S-1!x"),
            SyncProvider::GoogleDrive
        );
        assert_eq!(
            provider_of_sync_root_key("Nextcloud!S-1!x"),
            SyncProvider::Other("Nextcloud".into())
        );
    }
}
