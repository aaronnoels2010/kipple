// SPDX-License-Identifier: GPL-3.0-or-later
//! Windows: the volume GUID path (`\\?\Volume{…}\`) as the ID, `GetVolumeInformationW` for the
//! filesystem, `GetDriveTypeW` + `SHQueryRecycleBinW` for the Recycle Bin (S9).

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use kipple_core::identity::VolumeId;
use kipple_core::location::{StorageKind, TrashSupport, VolumeCaps};
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::Storage::FileSystem::{
    GetDriveTypeW, GetVolumeInformationW, GetVolumeNameForVolumeMountPointW, GetVolumePathNameW,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_SZ, RegCloseKey, RegEnumKeyExW, RegEnumValueW,
    RegOpenKeyExW,
};
use windows_sys::Win32::System::WindowsProgramming::{
    DRIVE_CDROM, DRIVE_FIXED, DRIVE_REMOTE, DRIVE_REMOVABLE,
};
use windows_sys::Win32::UI::Shell::{SHQUERYRBINFO, SHQueryRecycleBinW};

use crate::{Placeholders, PortResult, SyncRoot, SyncRoots, VolumeInfo, Volumes};

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

/// Online-only (cloud) file attributes: the content isn't on disk.
const FILE_ATTRIBUTE_OFFLINE: u32 = 0x0000_1000;
const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x0004_0000;
const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;

#[derive(Debug, Default, Clone)]
pub struct NativePlaceholders;

impl Placeholders for NativePlaceholders {
    fn is_placeholder(&self, path: &Path) -> PortResult<bool> {
        use std::os::windows::fs::MetadataExt;
        // Attributes only: reading them never hydrates the file.
        let attributes = std::fs::symlink_metadata(path)?.file_attributes();
        Ok(attributes
            & (FILE_ATTRIBUTE_OFFLINE
                | FILE_ATTRIBUTE_RECALL_ON_OPEN
                | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
            != 0)
    }
}

/// The sync roots registered with Windows' cloud files API (OneDrive, Dropbox, iCloud, Google
/// Drive…), from `HKLM\…\Explorer\SyncRootManager\<provider!SID!account>\UserSyncRoots`.
#[derive(Debug, Clone)]
pub struct NativeSyncRoots;

impl NativeSyncRoots {
    pub fn new(_home: PathBuf) -> Self {
        Self
    }
}

/// An open registry key, closed on drop.
struct Key(HKEY);

impl Key {
    fn open(parent: HKEY, subkey: &str) -> Option<Self> {
        let name: Vec<u16> = subkey.encode_utf16().chain([0]).collect();
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: `name` is NUL-terminated and `key` a valid out-pointer.
        (unsafe { RegOpenKeyExW(parent, name.as_ptr(), 0, KEY_READ, &mut key) } == ERROR_SUCCESS)
            .then_some(Self(key))
    }

    fn subkeys(&self) -> Vec<String> {
        let mut names = Vec::new();
        for index in 0.. {
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            // SAFETY: `buf` holds `len` code units; the other out-pointers are optional (null).
            let status = unsafe {
                RegEnumKeyExW(
                    self.0,
                    index,
                    buf.as_mut_ptr(),
                    &mut len,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if status != ERROR_SUCCESS {
                break;
            }
            names.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
        names
    }

    /// The string values (REG_SZ) under this key.
    fn string_values(&self) -> Vec<OsString> {
        let mut values = Vec::new();
        for index in 0.. {
            let mut name = [0u16; 256];
            let mut name_len = name.len() as u32;
            let mut data = [0u16; 1024];
            let mut data_len = (data.len() * 2) as u32;
            let mut kind = 0u32;
            // SAFETY: `name` holds `name_len` code units and `data` `data_len` bytes.
            let status = unsafe {
                RegEnumValueW(
                    self.0,
                    index,
                    name.as_mut_ptr(),
                    &mut name_len,
                    std::ptr::null(),
                    &mut kind,
                    data.as_mut_ptr().cast(),
                    &mut data_len,
                )
            };
            if status != ERROR_SUCCESS {
                break;
            }
            if kind == REG_SZ {
                values.push(from_wide(&data[..(data_len as usize / 2).min(data.len())]));
            }
        }
        values
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the key was opened by RegOpenKeyExW and is closed once.
        unsafe { RegCloseKey(self.0) };
    }
}

impl SyncRoots for NativeSyncRoots {
    fn sync_roots(&self) -> PortResult<Vec<SyncRoot>> {
        const MANAGER: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\SyncRootManager";
        // No key: nothing has ever registered a sync root on this PC.
        let Some(manager) = Key::open(HKEY_LOCAL_MACHINE, MANAGER) else {
            return Ok(Vec::new());
        };
        let mut roots = Vec::new();
        for provider_key in manager.subkeys() {
            let Some(user_roots) = Key::open(manager.0, &format!(r"{provider_key}\UserSyncRoots"))
            else {
                continue;
            };
            for path in user_roots.string_values() {
                let path = PathBuf::from(path);
                // Other users' roots are listed too; their folders aren't reachable from here.
                if path.is_dir() && !roots.iter().any(|r: &SyncRoot| r.path == path) {
                    roots.push(SyncRoot {
                        path,
                        provider: super::provider_of_sync_root_key(&provider_key),
                    });
                }
            }
        }
        Ok(roots)
    }
}
