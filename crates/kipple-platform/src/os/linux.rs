// SPDX-License-Identifier: GPL-3.0-or-later
//! Linux: the mount from `/proc/self/mountinfo` (matched by device and longest mount point), the
//! filesystem UUID from `/dev/disk/by-uuid`.

use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use kipple_core::identity::VolumeId;
use kipple_core::location::{StorageKind, TrashSupport, VolumeCaps};

use crate::{
    DownloadOrigins, Placeholders, PortError, PortResult, Screenshots, SyncRoot, SyncRoots,
    VolumeInfo, Volumes,
};

#[derive(Debug, Clone)]
pub struct NativeVolumes {
    /// The user's home folder: a volume on the same device uses the home Trash.
    home: PathBuf,
}

impl NativeVolumes {
    pub fn new(home: PathBuf) -> Self {
        Self { home }
    }
}

/// One line of /proc/self/mountinfo.
#[derive(Debug)]
struct Mount {
    id: u64,
    dev: (u32, u32),
    mount_point: PathBuf,
    /// Read-only for this mount or for the whole filesystem.
    read_only: bool,
    fs_type: String,
    source: String,
}

/// Undoes mountinfo's octal escapes (`\040` for a space).
fn unescape(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && bytes[i + 1..i + 4].iter().all(u8::is_ascii_digit)
            && let Ok(value) = u8::from_str_radix(&field[i + 1..i + 4], 8)
        {
            out.push(value);
            i += 4;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `id parent major:minor root mount_point options [optional fields…] - fstype source super_options`
fn parse_mountinfo(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|line| {
            let (before, after) = line.split_once(" - ")?;
            let fields: Vec<&str> = before.split(' ').collect();
            let (major, minor) = fields.get(2)?.split_once(':')?;
            let mut rest = after.split(' ');
            let fs_type = rest.next()?.to_string();
            let source = unescape(rest.next()?);
            let super_ro = rest
                .next()
                .is_some_and(|opts| opts.split(',').any(|o| o == "ro"));
            Some(Mount {
                id: fields.first()?.parse().ok()?,
                dev: (major.parse().ok()?, minor.parse().ok()?),
                mount_point: PathBuf::from(unescape(fields.get(4)?)),
                read_only: super_ro || fields.get(5)?.split(',').any(|o| o == "ro"),
                fs_type,
                source,
            })
        })
        .collect()
}

/// The mount ID of the mount holding `path` (Linux 5.8+): unlike the device number, it also
/// identifies btrfs subvolumes correctly.
fn mount_id(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: an all-zero statx is a valid value for this plain C struct.
    let mut stx: libc::statx = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a NUL-terminated path and `stx` a valid out-pointer.
    let status =
        unsafe { libc::statx(libc::AT_FDCWD, c.as_ptr(), 0, libc::STATX_MNT_ID, &mut stx) };
    (status == 0 && stx.stx_mask & libc::STATX_MNT_ID != 0).then_some(stx.stx_mnt_id)
}

/// The filesystem UUID of a block device like `/dev/sda1`, from the udev symlinks.
fn fs_uuid(source: &str) -> Option<String> {
    let device = std::fs::canonicalize(source).ok()?;
    std::fs::read_dir("/dev/disk/by-uuid")
        .ok()?
        .flatten()
        .find_map(|entry| {
            (std::fs::canonicalize(entry.path()).ok()? == device)
                .then(|| entry.file_name().to_string_lossy().into_owned())
        })
}

/// The device of the user's Trash (`$XDG_DATA_HOME/Trash`, or its nearest existing parent):
/// files on that device go to the home Trash (S9).
fn home_trash_device(home: &Path) -> Option<u64> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".local/share"));
    data_home
        .join("Trash")
        .ancestors()
        .find_map(|dir| std::fs::metadata(dir).ok())
        .map(|m| m.dev())
}

impl Volumes for NativeVolumes {
    fn volume_of(&self, path: &Path) -> PortResult<VolumeInfo> {
        let meta = std::fs::metadata(path)?;
        let dev = (libc::major(meta.dev()), libc::minor(meta.dev()));
        let path = std::fs::canonicalize(path)?;
        let mounts = parse_mountinfo(&std::fs::read_to_string("/proc/self/mountinfo")?);
        let by_id = mount_id(&path).and_then(|id| mounts.iter().position(|m| m.id == id));
        let at = by_id
            .or_else(|| {
                mounts
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.dev == dev && path.starts_with(&m.mount_point))
                    .max_by_key(|(_, m)| m.mount_point.components().count())
                    .map(|(i, _)| i)
            })
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no mount for this path"))?;
        let mount = mounts.into_iter().nth(at).expect("index from this list");
        let network = super::is_network_fs(&mount.fs_type);
        // Without a filesystem UUID: the device it came from and its type (device numbers alone
        // get reused by the next stick plugged in).
        let id = fs_uuid(&mount.source)
            .unwrap_or_else(|| format!("src:{}:{}", mount.source, mount.fs_type));
        let trash = if network || mount.read_only {
            TrashSupport::No
        } else if home_trash_device(&self.home) == Some(meta.dev()) {
            TrashSupport::Yes
        } else {
            // `$topdir/.Trash-$uid` may still work; the Trash port (K-4.6) checks it.
            TrashSupport::Unknown
        };
        let fs = mount.fs_type.to_ascii_lowercase();
        Ok(VolumeInfo {
            id: VolumeId(id),
            caps: VolumeCaps {
                trash,
                case_sensitive: !matches!(
                    fs.as_str(),
                    "vfat" | "msdos" | "exfat" | "ntfs" | "ntfs3" | "fuseblk"
                ),
                stable_file_ids: !network && super::has_stable_file_ids(&fs),
                storage: if network {
                    StorageKind::Network
                } else {
                    StorageKind::Unknown
                },
            },
            mount_point: mount.mount_point,
            read_only: mount.read_only,
            fs_type: mount.fs_type,
        })
    }
}

/// Linux has no online-only files (FUSE and rclone mounts count as network drives instead).
#[derive(Debug, Default, Clone)]
pub struct NativePlaceholders;

impl Placeholders for NativePlaceholders {
    fn is_placeholder(&self, _path: &Path) -> PortResult<bool> {
        Err(PortError::Unsupported)
    }
}

/// Linux sync clients don't register their folders anywhere Kipple can read.
#[derive(Debug, Clone)]
pub struct NativeSyncRoots;

impl NativeSyncRoots {
    pub fn new(_home: PathBuf) -> Self {
        Self
    }
}

impl SyncRoots for NativeSyncRoots {
    fn sync_roots(&self) -> PortResult<Vec<SyncRoot>> {
        Err(PortError::Unsupported)
    }
}

/// `user.xdg.origin.url` (freedesktop "Common Extended Attributes"): written by Chromium-based
/// browsers and by `curl --xattr` and `wget --xattr`; Firefox writes nothing.
#[derive(Debug, Default, Clone)]
pub struct NativeDownloadOrigins;

impl DownloadOrigins for NativeDownloadOrigins {
    fn origin_of(&self, path: &Path) -> PortResult<Option<String>> {
        let Some(mut value) = super::unix::xattr(path, c"user.xdg.origin.url")? else {
            return Ok(None);
        };
        if value.last() == Some(&0) {
            value.pop();
        }
        Ok(String::from_utf8(value).ok().filter(|url| !url.is_empty()))
    }
}

/// Screenshots by where GNOME and Spectacle save them (a Screenshots folder in Pictures, in
/// any of the languages in `screenshot_names`) or by the names they give them, wherever the
/// file is now. Both are worked out from the path alone.
#[derive(Debug, Clone)]
pub struct NativeScreenshots {
    folders: Vec<PathBuf>,
}

impl NativeScreenshots {
    /// Reads `user-dirs.dirs` once, for a Pictures folder with another name ("Afbeeldingen").
    pub fn new(home: PathBuf) -> Self {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        Self::with_config(&home, &config)
    }

    fn with_config(home: &Path, config: &Path) -> Self {
        // Items have canonical paths: folders reached through a link must match them.
        let home = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
        let pictures = std::fs::read_to_string(config.join("user-dirs.dirs"))
            .ok()
            .and_then(|text| super::screenshot_names::pictures_dir(&text, &home));
        let folders = super::screenshot_names::screenshot_folders(&home, pictures.as_deref())
            .into_iter()
            .map(|folder| std::fs::canonicalize(&folder).unwrap_or(folder))
            .collect();
        Self { folders }
    }
}

impl Screenshots for NativeScreenshots {
    fn is_screenshot(&self, path: &Path) -> PortResult<bool> {
        let in_folder = self
            .folders
            .iter()
            .any(|folder| kipple_core::paths::is_within(folder, path, true));
        Ok(in_folder
            || path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(super::screenshot_names::is_screenshot_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshots_folders_follow_the_localised_pictures_folder() {
        let home = tempfile::tempdir().unwrap();
        let home = std::fs::canonicalize(home.path()).unwrap();
        let config = home.join(".config");
        #[allow(clippy::disallowed_methods)] // test fixture
        {
            std::fs::create_dir_all(&config).unwrap();
            std::fs::write(
                config.join("user-dirs.dirs"),
                "XDG_PICTURES_DIR=\"$HOME/Afbeeldingen\"\n",
            )
            .unwrap();
        }
        let screenshots = NativeScreenshots::with_config(&home, &config);
        let check = |path: &str| screenshots.is_screenshot(&home.join(path)).unwrap();
        assert!(check("Afbeeldingen/Schermafdrukken/holiday.png"));
        assert!(check("Afbeeldingen/Screenshots/holiday.png"));
        assert!(check("Pictures/Screenshots/holiday.png"));
        assert!(check("Downloads/Schermafdruk van 2026-10-21 10-20-30.png"));
        assert!(!check("Afbeeldingen/holiday.png"));
        assert!(
            !check("Afbeeldingen/schermafdrukken/holiday.png"),
            "Linux names are case-sensitive"
        );
    }

    #[test]
    fn parses_mountinfo_lines() {
        let text = "\
22 1 259:2 / / rw,relatime shared:1 - ext4 /dev/nvme0n1p2 rw
36 22 8:17 / /media/zoe/My\\040USB ro,nosuid - vfat /dev/sdb1 ro,fmask=0022
40 22 0:51 / /mnt/share rw - cifs //nas/share rw,vers=3.0
41 22 0:52 /@home /home rw shared:5 master:2 - btrfs /dev/sda2 ro,space_cache=v2";
        let mounts = parse_mountinfo(text);
        assert_eq!(mounts.len(), 4);
        assert_eq!(mounts[0].dev, (259, 2));
        assert_eq!(mounts[0].id, 22);
        assert_eq!(mounts[3].mount_point, Path::new("/home"));
        assert!(
            mounts[3].read_only,
            "read-only for the whole filesystem (super options)"
        );
        assert_eq!(mounts[1].mount_point, Path::new("/media/zoe/My USB"));
        assert!(mounts[1].read_only && !mounts[0].read_only);
        assert_eq!(
            (mounts[2].fs_type.as_str(), mounts[2].source.as_str()),
            ("cifs", "//nas/share")
        );
    }
}
