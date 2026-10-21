// SPDX-License-Identifier: GPL-3.0-or-later
//! macOS: `statfs` for the mount, `getattrlist(ATTR_VOL_UUID)` for a stable volume ID.

use std::ffi::{CStr, CString, OsStr};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use kipple_core::identity::VolumeId;
use kipple_core::location::{StorageKind, TrashSupport, VolumeCaps};

use crate::{PortResult, VolumeInfo, Volumes};

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
    (status == 0 && reply.uuid != [0; 16]).then_some(reply.uuid)
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
            .unwrap_or_else(|| format!("mount:{}", mount_point.display()));
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
