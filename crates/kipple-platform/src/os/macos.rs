// SPDX-License-Identifier: GPL-3.0-or-later
//! macOS: `statfs` for the mount, `getattrlist(ATTR_VOL_UUID)` for a stable volume ID.

use std::ffi::{CStr, CString, OsStr};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use kipple_core::identity::VolumeId;
use kipple_core::location::{StorageKind, TrashSupport, VolumeCaps};

use crate::{Placeholders, PortResult, SyncProvider, SyncRoot, SyncRoots, VolumeInfo, Volumes};

#[derive(Debug, Default, Clone)]
pub struct NativeVolumes;

impl NativeVolumes {
    /// `home` is only needed on Linux (to find the home Trash); same signature everywhere.
    pub fn new(_home: std::path::PathBuf) -> Self {
        Self
    }
}

fn c_path(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
}

fn c_chars(chars: &[libc::c_char]) -> &[u8] {
    // SAFETY: statfs fills these fixed-size fields with NUL-terminated strings.
    unsafe { CStr::from_ptr(chars.as_ptr()) }.to_bytes()
}

/// The volume UUID of the volume mounted at `mount`, if the filesystem has one.
fn volume_uuid(mount: &CStr) -> Option<[u8; 16]> {
    #[repr(C)]
    struct Reply {
        length: u32,
        uuid: [u8; 16],
    }
    let mut request = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: 0,
        volattr: libc::ATTR_VOL_INFO | libc::ATTR_VOL_UUID,
        dirattr: 0,
        fileattr: 0,
        forkattr: 0,
    };
    let mut reply = Reply {
        length: 0,
        uuid: [0; 16],
    };
    // SAFETY: `request` asks for exactly the volume UUID, which the kernel writes into `reply`
    // (length + 16 bytes); the buffer size passed is the size of `reply`.
    let status = unsafe {
        libc::getattrlist(
            mount.as_ptr(),
            (&raw mut request).cast(),
            (&raw mut reply).cast(),
            size_of::<Reply>(),
            0,
        )
    };
    // `length` covers itself plus the UUID when the filesystem returned one.
    (status == 0 && reply.length >= 20 && reply.uuid != [0; 16]).then_some(reply.uuid)
}

fn uuid_string(b: [u8; 16]) -> String {
    format!(
        "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
        b[6],
        b[7],
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    )
}

impl Volumes for NativeVolumes {
    fn volume_of(&self, path: &Path) -> PortResult<VolumeInfo> {
        let c = c_path(path)?;
        // SAFETY: an all-zero statfs is a valid value for this plain C struct.
        let mut st: libc::statfs = unsafe { std::mem::zeroed() };
        // SAFETY: `c` is a NUL-terminated path and `st` a valid out-pointer.
        if unsafe { libc::statfs(c.as_ptr(), &mut st) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
        let mount_bytes = c_chars(&st.f_mntonname);
        let mount_point = PathBuf::from(OsStr::from_bytes(mount_bytes));
        let fs_type = String::from_utf8_lossy(c_chars(&st.f_fstypename)).into_owned();
        let flags = st.f_flags as i64;
        let read_only = flags & libc::MNT_RDONLY as i64 != 0;
        let local = flags & libc::MNT_LOCAL as i64 != 0;
        let id = c_path(&mount_point)
            .ok()
            .and_then(|m| volume_uuid(&m))
            .map(uuid_string)
            // No UUID (some FAT and network volumes): where it is mounted, which holds while it
            // stays mounted there.
            .unwrap_or_else(|| format!("mount:{}:{fs_type}", mount_point.display()));
        // SAFETY: `c` is a NUL-terminated path; pathconf only reads it.
        let case_sensitive = unsafe { libc::pathconf(c.as_ptr(), libc::_PC_CASE_SENSITIVE) } == 1;
        let network = !local || super::is_network_fs(&fs_type);
        Ok(VolumeInfo {
            id: VolumeId(id),
            mount_point,
            caps: VolumeCaps {
                // The full S9 rule (does NSTrashDirectory work there?) comes with the Trash port.
                trash: if network || read_only {
                    TrashSupport::No
                } else {
                    TrashSupport::Unknown
                },
                case_sensitive,
                stable_file_ids: !network && super::has_stable_file_ids(&fs_type),
                storage: if network {
                    StorageKind::Network
                } else {
                    StorageKind::Unknown
                },
            },
            fs_type,
            read_only,
        })
    }
}

/// `SF_DATALESS`: the file's content lives in the cloud (iCloud, File Provider apps).
const SF_DATALESS: u32 = 0x4000_0000;

#[derive(Debug, Default, Clone)]
pub struct NativePlaceholders;

impl Placeholders for NativePlaceholders {
    fn is_placeholder(&self, path: &Path) -> PortResult<bool> {
        use std::os::macos::fs::MetadataExt;
        // Metadata only: reading the flags never downloads anything.
        let flags = std::fs::symlink_metadata(path)?.st_flags();
        Ok(is_placeholder(path.file_name().unwrap_or_default(), flags))
    }
}

/// From a file's name and `st_flags`.
pub(crate) fn is_placeholder(name: &OsStr, flags: u32) -> bool {
    // Older iCloud versions left `.Name.ext.icloud` stub files instead of dataless ones.
    let stub = name
        .to_str()
        .is_some_and(|n| n.starts_with('.') && n.ends_with(".icloud"));
    stub || flags & SF_DATALESS != 0
}

pub(crate) fn never_download_placeholders() -> PortResult<()> {
    // <sys/resource.h>; libc doesn't bind them.
    const IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES: libc::c_int = 3;
    const IOPOL_SCOPE_PROCESS: libc::c_int = 0;
    const IOPOL_MATERIALIZE_DATALESS_FILES_OFF: libc::c_int = 1;
    unsafe extern "C" {
        fn setiopolicy_np(
            iotype: libc::c_int,
            scope: libc::c_int,
            policy: libc::c_int,
        ) -> libc::c_int;
    }
    // SAFETY: plain integer arguments; the call only changes this process's I/O policy.
    let status = unsafe {
        setiopolicy_np(
            IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES,
            IOPOL_SCOPE_PROCESS,
            IOPOL_MATERIALIZE_DATALESS_FILES_OFF,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error().into())
    }
}

/// iCloud Drive and every File Provider folder (`~/Library/CloudStorage/*`).
#[derive(Debug, Clone)]
pub struct NativeSyncRoots {
    home: PathBuf,
}

impl NativeSyncRoots {
    pub fn new(home: PathBuf) -> Self {
        Self { home }
    }
}

impl SyncRoots for NativeSyncRoots {
    fn sync_roots(&self) -> PortResult<Vec<SyncRoot>> {
        let library = self.home.join("Library");
        let mut roots = Vec::new();
        let icloud = library.join("Mobile Documents/com~apple~CloudDocs");
        if icloud.is_dir() {
            roots.push(SyncRoot {
                path: icloud,
                provider: SyncProvider::ICloudDrive,
            });
        }
        // Missing on Macs that never used a File Provider app: that's "none", not an error.
        if let Ok(entries) = std::fs::read_dir(library.join("CloudStorage")) {
            for entry in entries.flatten() {
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    roots.push(SyncRoot {
                        path: entry.path(),
                        provider: super::provider_of_cloud_storage(&name),
                    });
                }
            }
        }
        Ok(roots)
    }
}
