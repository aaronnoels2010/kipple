// SPDX-License-Identifier: GPL-3.0-or-later
//! Windows: the volume GUID path (`\\?\Volume{…}\`) as the ID, `GetVolumeInformationW` for the
//! filesystem, `GetDriveTypeW` + `SHQueryRecycleBinW` for the Recycle Bin (S9).

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use kipple_core::identity::VolumeId;
use kipple_core::location::{StorageKind, TrashSupport, VolumeCaps};
use windows_sys::Win32::Storage::FileSystem::{
    GetDriveTypeW, GetVolumeInformationW, GetVolumeNameForVolumeMountPointW, GetVolumePathNameW,
};
use windows_sys::Win32::System::WindowsProgramming::{
    DRIVE_CDROM, DRIVE_FIXED, DRIVE_REMOTE, DRIVE_REMOVABLE,
};
use windows_sys::Win32::UI::Shell::{SHQUERYRBINFO, SHQueryRecycleBinW};

use crate::{PortResult, VolumeInfo, Volumes};

#[derive(Debug, Default, Clone)]
pub struct NativeVolumes;

impl NativeVolumes {
    /// `home` is only needed on Linux (to find the home Trash); same signature everywhere.
    pub fn new(_home: std::path::PathBuf) -> Self {
        Self
    }
}

const FILE_READ_ONLY_VOLUME: u32 = 0x0008_0000;
const MAX: usize = 1024;

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain([0]).collect()
}

fn from_wide(buf: &[u16]) -> OsString {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    OsString::from_wide(&buf[..end])
}

impl Volumes for NativeVolumes {
    fn volume_of(&self, path: &Path) -> PortResult<VolumeInfo> {
        let path_w = wide(path);
        let mut mount = [0u16; MAX];
        // SAFETY: `path_w` is NUL-terminated; `mount` has room for MAX code units.
        if unsafe { GetVolumePathNameW(path_w.as_ptr(), mount.as_mut_ptr(), MAX as u32) } == 0 {
            return Err(io::Error::last_os_error().into());
        }
        let mount_point = PathBuf::from(from_wide(&mount));
        let mount_w = wide(&mount_point);

        let mut fs_name = [0u16; 64];
        let (mut serial, mut flags) = (0u32, 0u32);
        // SAFETY: `mount_w` is NUL-terminated; the out-buffers and their sizes match.
        let ok = unsafe {
            GetVolumeInformationW(
                mount_w.as_ptr(),
                std::ptr::null_mut(),
                0,
                &mut serial,
                std::ptr::null_mut(),
                &mut flags,
                fs_name.as_mut_ptr(),
                fs_name.len() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error().into());
        }
        let fs_type = from_wide(&fs_name).to_string_lossy().into_owned();

        let mut guid = [0u16; 64];
        // SAFETY: `mount_w` is a NUL-terminated mount point with a trailing backslash, as required.
        let has_guid = unsafe {
            GetVolumeNameForVolumeMountPointW(
                mount_w.as_ptr(),
                guid.as_mut_ptr(),
                guid.len() as u32,
            )
        } != 0;
        let id = if has_guid {
            from_wide(&guid).to_string_lossy().into_owned()
        } else {
            // Network drives have no volume GUID: the serial plus where it's mounted.
            format!("serial:{serial:08X}:{}", mount_point.display())
        };

        // SAFETY: `mount_w` is NUL-terminated.
        let drive = unsafe { GetDriveTypeW(mount_w.as_ptr()) };
        let read_only = flags & FILE_READ_ONLY_VOLUME != 0;
        let trash = match drive {
            DRIVE_REMOTE | DRIVE_REMOVABLE | DRIVE_CDROM => TrashSupport::No,
            DRIVE_FIXED if !read_only => {
                let mut info = SHQUERYRBINFO {
                    cbSize: size_of::<SHQUERYRBINFO>() as u32,
                    i64Size: 0,
                    i64NumItems: 0,
                };
                // SAFETY: `mount_w` is NUL-terminated and `info.cbSize` is set as required.
                let hr = unsafe { SHQueryRecycleBinW(mount_w.as_ptr(), &mut info) };
                if hr == 0 {
                    TrashSupport::Yes
                } else {
                    TrashSupport::Unknown
                }
            }
            _ => TrashSupport::No,
        };
        let network = drive == DRIVE_REMOTE;
        Ok(VolumeInfo {
            id: VolumeId(id),
            mount_point,
            caps: VolumeCaps {
                trash,
                // Windows compares names case-insensitively, whatever the filesystem supports.
                case_sensitive: false,
                stable_file_ids: !network && super::has_stable_file_ids(&fs_type),
                storage: match drive {
                    DRIVE_REMOTE => StorageKind::Network,
                    DRIVE_REMOVABLE => StorageKind::Removable,
                    _ => StorageKind::Unknown,
                },
            },
            fs_type,
            read_only,
        })
    }
}
