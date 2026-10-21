// SPDX-License-Identifier: GPL-3.0-or-later
//! Linux: the mount from `/proc/self/mountinfo` (matched by device and longest mount point), the
//! filesystem UUID from `/dev/disk/by-uuid`.

use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use kipple_core::identity::VolumeId;
use kipple_core::location::{StorageKind, TrashSupport, VolumeCaps};

use crate::{PortResult, VolumeInfo, Volumes};

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
    dev: (u32, u32),
    mount_point: PathBuf,
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

fn parse_mountinfo(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|line| {
            let (before, after) = line.split_once(" - ")?;
            let fields: Vec<&str> = before.split(' ').collect();
            let (major, minor) = fields.get(2)?.split_once(':')?;
            let mut rest = after.split(' ');
            Some(Mount {
                dev: (major.parse().ok()?, minor.parse().ok()?),
                mount_point: PathBuf::from(unescape(fields.get(4)?)),
                read_only: fields.get(5)?.split(',').any(|o| o == "ro"),
                fs_type: rest.next()?.to_string(),
                source: unescape(rest.next()?),
            })
        })
        .collect()
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

impl Volumes for NativeVolumes {
    fn volume_of(&self, path: &Path) -> PortResult<VolumeInfo> {
        let meta = std::fs::metadata(path)?;
        let dev = (libc::major(meta.dev()), libc::minor(meta.dev()));
        let path = std::fs::canonicalize(path)?;
        let mounts = parse_mountinfo(&std::fs::read_to_string("/proc/self/mountinfo")?);
        let mount = mounts
            .into_iter()
            .filter(|m| m.dev == dev && path.starts_with(&m.mount_point))
            .max_by_key(|m| m.mount_point.components().count())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no mount for this path"))?;
        let network = super::is_network_fs(&mount.fs_type);
        let id = fs_uuid(&mount.source).unwrap_or_else(|| format!("dev:{}:{}", dev.0, dev.1));
        let same_device_as_home =
            std::fs::metadata(&self.home).is_ok_and(|home| home.dev() == meta.dev());
        let trash = if network || mount.read_only {
            TrashSupport::No
        } else if same_device_as_home {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mountinfo_lines() {
        let text = "\
22 1 259:2 / / rw,relatime shared:1 - ext4 /dev/nvme0n1p2 rw
36 22 8:17 / /media/zoe/My\\040USB ro,nosuid - vfat /dev/sdb1 ro,fmask=0022
40 22 0:51 / /mnt/share rw - cifs //nas/share rw,vers=3.0";
        let mounts = parse_mountinfo(text);
        assert_eq!(mounts.len(), 3);
        assert_eq!(mounts[0].dev, (259, 2));
        assert_eq!(mounts[1].mount_point, Path::new("/media/zoe/My USB"));
        assert!(mounts[1].read_only && !mounts[0].read_only);
        assert_eq!(
            (mounts[2].fs_type.as_str(), mounts[2].source.as_str()),
            ("cifs", "//nas/share")
        );
    }
}
