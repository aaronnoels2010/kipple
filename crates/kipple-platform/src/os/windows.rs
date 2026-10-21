// SPDX-License-Identifier: GPL-3.0-or-later
//! Windows: the volume GUID path (`\\?\Volume{…}\`) as the ID, `GetVolumeInformationW` for the
//! filesystem, `GetDriveTypeW` for the kind of drive (network and removable: no Recycle Bin, S9).

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use kipple_core::identity::{FileId, VolumeId};
use kipple_core::location::{StorageKind, TrashSupport, VolumeCaps};
use kipple_core::time::Timestamp;
use windows_sys::Wdk::Storage::FileSystem::RtlSetProcessPlaceholderCompatibilityMode;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_INVALID_FUNCTION, ERROR_INVALID_LEVEL,
    ERROR_INVALID_NAME, ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES, ERROR_NO_MORE_ITEMS,
    ERROR_NOT_SUPPORTED, ERROR_SUCCESS, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_NO_RECALL, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_GENERIC_READ, FILE_ID_BOTH_DIR_INFO, FILE_LIST_DIRECTORY, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FileIdBothDirectoryInfo, GetDriveTypeW,
    GetFileInformationByHandleEx, GetVolumeInformationW, GetVolumeNameForVolumeMountPointW,
    GetVolumePathNameW, OPEN_EXISTING,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_SZ, RegCloseKey, RegEnumKeyExW, RegEnumValueW,
    RegOpenKeyExW,
};
use windows_sys::Win32::System::WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOTE, DRIVE_REMOVABLE};

use crate::{
    DirEntry, Directories, DownloadOrigins, EntryKind, Placeholders, PortResult, Screenshots,
    SyncRoot, SyncRoots, VolumeInfo, Volumes,
};

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

/// `path` with links (symlinks, junctions, mapped folders) resolved and without the `\\?\`
/// prefix `canonicalize` adds: `C:\data` linked to `\\nas\share` must be judged as the share.
pub(crate) fn resolve(path: &Path) -> io::Result<PathBuf> {
    use std::path::{Component, Prefix};
    let canonical = std::fs::canonicalize(path)?;
    let mut components = canonical.components();
    let plain = match components.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::VerbatimDisk(drive) => format!("{}:\\", char::from(drive)),
            Prefix::VerbatimUNC(server, share) => {
                format!(
                    r"\\{}\{}\",
                    server.to_string_lossy(),
                    share.to_string_lossy()
                )
            }
            _ => return Ok(canonical),
        },
        _ => return Ok(canonical),
    };
    Ok(components.fold(PathBuf::from(plain), |acc, c| match c {
        Component::RootDir => acc,
        other => acc.join(other.as_os_str()),
    }))
}

impl Volumes for NativeVolumes {
    fn volume_of(&self, path: &Path) -> PortResult<VolumeInfo> {
        let path_w = wide(&resolve(path)?);
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
        // A fixed drive usually has a Recycle Bin, but it can be switched off for the drive or be
        // too small for a file, and then deleting is permanent: the Trash port (K-4.6) checks
        // those settings. Until then: unknown, which counts as no.
        let trash = match drive {
            DRIVE_FIXED if !read_only => TrashSupport::Unknown,
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
            // Too long or unreadable: skip that one, keep going until there are no more.
            if status == ERROR_NO_MORE_ITEMS {
                break;
            }
            if status != ERROR_SUCCESS {
                continue;
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
            // Too long or unreadable: skip that one, keep going until there are no more.
            if status == ERROR_NO_MORE_ITEMS {
                break;
            }
            if status != ERROR_SUCCESS {
                continue;
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

/// Lists a folder with one bulk call per 64 KiB of entries (`FileIdBothDirectoryInfo`): name,
/// size, times, attributes and file ID without opening any file (spike S4: faster than plain
/// enumeration). Falls back to plain enumeration, without file IDs or ChangeTime, where the
/// call isn't supported (some network shares).
#[derive(Debug, Default, Clone)]
pub struct NativeDirectories;

impl Directories for NativeDirectories {
    fn list(&self, dir: &Path) -> PortResult<Vec<DirEntry>> {
        match list_by_handle(dir)? {
            Some(entries) => Ok(entries),
            None => Ok(list_plain(dir)?),
        }
    }
}

/// `path` in the `\\?\` form, NUL-terminated: without it, Windows refuses paths longer than
/// 260 characters unless long paths are switched on (std adds it too). Paths here are absolute
/// and canonical.
fn verbatim(path: &Path) -> Vec<u16> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    let starts = |prefix: &str| {
        let prefix: Vec<u16> = prefix.encode_utf16().collect();
        wide.starts_with(&prefix)
    };
    // The prefix to add, and how many leading characters it replaces.
    let (prefix, skip) = if starts(r"\\?\") || starts(r"\\.\") {
        ("", 0)
    } else if starts(r"\\") {
        // \\server\share → \\?\UNC\server\share
        (r"\\?\UNC", 1)
    } else if wide.get(1) == Some(&u16::from(b':')) {
        (r"\\?\", 0)
    } else {
        ("", 0)
    };
    let mut out: Vec<u16> = prefix.encode_utf16().collect();
    out.extend(wide[skip..].iter().map(|&unit| {
        // The verbatim form takes no forward slashes.
        if unit == u16::from(b'/') {
            u16::from(b'\\')
        } else {
            unit
        }
    }));
    out.push(0);
    out
}

/// A handle, closed on drop.
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateFileW and is closed once.
        unsafe { CloseHandle(self.0) };
    }
}

/// 100 ns ticks since 1601 to nanoseconds since 1970.
fn filetime(ticks: i64) -> Timestamp {
    Timestamp(
        ticks
            .saturating_sub(116_444_736_000_000_000)
            .saturating_mul(100),
    )
}

fn kind_of(attributes: u32, reparse_tag: u32) -> EntryKind {
    // Symlinks, junctions and mount points are "name surrogates"; other reparse points (cloud
    // files, deduplicated files) are ordinary files and folders.
    const NAME_SURROGATE: u32 = 0x2000_0000;
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 && reparse_tag & NAME_SURROGATE != 0 {
        EntryKind::Symlink
    } else if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        EntryKind::Dir
    } else {
        EntryKind::File
    }
}

fn is_placeholder_attributes(attributes: u32) -> bool {
    attributes
        & (FILE_ATTRIBUTE_OFFLINE
            | FILE_ATTRIBUTE_RECALL_ON_OPEN
            | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
        != 0
}

/// `None` where the filesystem doesn't support the bulk call: then use [`list_plain`].
fn list_by_handle(dir: &Path) -> io::Result<Option<Vec<DirEntry>>> {
    let path = verbatim(dir);
    // SAFETY: `path` is NUL-terminated and outlives the call; listing access only.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            FILE_LIST_DIRECTORY,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let handle = Handle(handle);
    // 8-byte aligned, as the records are.
    let mut buffer = vec![0u64; 8192];
    let size = buffer.len() * 8;
    let mut entries = Vec::new();
    let mut first = true;
    loop {
        // SAFETY: a directory handle and a writable buffer of `size` bytes.
        let ok = unsafe {
            GetFileInformationByHandleEx(
                handle.0,
                FileIdBothDirectoryInfo,
                buffer.as_mut_ptr().cast(),
                u32::try_from(size).unwrap_or(u32::MAX),
            )
        };
        if ok == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                return Ok(Some(entries));
            }
            // Filesystems without the call (some network redirectors) say so on the first call.
            // Anything else fails the listing (the folder keeps its Items): a plain listing has
            // no file IDs, so every entry would look changed and lose what was enriched.
            let unsupported = matches!(
                error.raw_os_error().map(|code| code as u32),
                Some(
                    ERROR_INVALID_PARAMETER
                        | ERROR_NOT_SUPPORTED
                        | ERROR_INVALID_LEVEL
                        | ERROR_INVALID_FUNCTION
                )
            );
            return if first && unsupported {
                Ok(None)
            } else {
                Err(error)
            };
        }
        first = false;
        let base = buffer.as_ptr().cast::<u8>();
        let mut offset = 0usize;
        loop {
            // A driver bug must not make us read past the buffer.
            let header = std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
            if !offset.is_multiple_of(8) || offset + size_of::<FILE_ID_BOTH_DIR_INFO>() > size {
                return Err(io::ErrorKind::InvalidData.into());
            }
            // SAFETY: the call filled the buffer with a chain of FILE_ID_BOTH_DIR_INFO records,
            // each 8-byte aligned, starting at 0 and linked by NextEntryOffset; each one holds
            // its whole header and FileNameLength bytes of name.
            let info = unsafe { base.add(offset).cast::<FILE_ID_BOTH_DIR_INFO>().read() };
            if offset + header + info.FileNameLength as usize > size {
                return Err(io::ErrorKind::InvalidData.into());
            }
            // SAFETY: as above; the name was just checked to lie inside the buffer.
            let name = unsafe {
                let record = base.add(offset).cast::<FILE_ID_BOTH_DIR_INFO>();
                std::slice::from_raw_parts(
                    (&raw const (*record).FileName).cast::<u16>(),
                    info.FileNameLength as usize / 2,
                )
            };
            if name != [u16::from(b'.')] && name != [u16::from(b'.'); 2] {
                let kind = kind_of(info.FileAttributes, info.EaSize);
                entries.push(DirEntry {
                    name: OsString::from_wide(name),
                    kind,
                    size: if kind == EntryKind::Dir {
                        0
                    } else {
                        u64::try_from(info.EndOfFile).unwrap_or(0)
                    },
                    modified: filetime(info.LastWriteTime),
                    changed: Some(filetime(info.ChangeTime)),
                    created: Some(filetime(info.CreationTime)),
                    file_id: Some(FileId(u128::from(info.FileId as u64))),
                    hidden: info.FileAttributes & FILE_ATTRIBUTE_HIDDEN != 0,
                    placeholder: is_placeholder_attributes(info.FileAttributes),
                    mount_point: false,
                });
            }
            if info.NextEntryOffset == 0 {
                break;
            }
            offset += info.NextEntryOffset as usize;
        }
    }
}

/// Plain enumeration: what `FindFirstFile` reports, no ChangeTime or file ID.
fn list_plain(dir: &Path) -> io::Result<Vec<DirEntry>> {
    use std::os::windows::fs::MetadataExt;
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let meta = match entry.metadata() {
            Ok(meta) => meta,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        let attributes = meta.file_attributes();
        let kind = if meta.file_type().is_symlink() {
            EntryKind::Symlink
        } else if meta.is_dir() {
            EntryKind::Dir
        } else {
            EntryKind::File
        };
        let ticks = |t: u64| filetime(i64::try_from(t).unwrap_or(i64::MAX));
        entries.push(DirEntry {
            name: entry.file_name(),
            kind,
            size: if kind == EntryKind::Dir {
                0
            } else {
                meta.file_size()
            },
            modified: ticks(meta.last_write_time()),
            changed: None,
            created: Some(ticks(meta.creation_time())),
            file_id: None,
            hidden: attributes & FILE_ATTRIBUTE_HIDDEN != 0,
            placeholder: is_placeholder_attributes(attributes),
            mount_point: false,
        });
    }
    Ok(entries)
}

/// The `Zone.Identifier` stream browsers and other downloaders put next to a file ("Mark of
/// the Web"), read without recalling a cloud file or following a link.
#[derive(Debug, Default, Clone)]
pub struct NativeDownloadOrigins;

impl DownloadOrigins for NativeDownloadOrigins {
    fn origin_of(&self, path: &Path) -> PortResult<Option<String>> {
        use std::io::Read;
        use std::os::windows::io::FromRawHandle;
        let mut stream = path.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        let name = verbatim(Path::new(&stream));
        // SAFETY: `name` is NUL-terminated and outlives the call; read access only. NO_RECALL
        // keeps a cloud file in the cloud; OPEN_REPARSE_POINT opens a link, not its target.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                FILE_GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_NO_RECALL | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            let error = io::Error::last_os_error();
            return match error.raw_os_error().map(|code| code as u32) {
                // No stream, or a filesystem without streams (FAT, exFAT, some shares): no
                // record, as long as the file itself is there.
                Some(
                    ERROR_FILE_NOT_FOUND
                    | ERROR_INVALID_NAME
                    | ERROR_INVALID_PARAMETER
                    | ERROR_NOT_SUPPORTED,
                ) => {
                    std::fs::symlink_metadata(path)?;
                    Ok(None)
                }
                _ => Err(error.into()),
            };
        }
        // SAFETY: a handle just opened; the File owns it from here and closes it.
        let file = unsafe { std::fs::File::from_raw_handle(handle) };
        // A few lines of text; anything past 64 KiB isn't a Zone.Identifier.
        let mut text = Vec::new();
        file.take(64 * 1024).read_to_end(&mut text)?;
        Ok(super::zone_host_url(&text))
    }
}

/// Screenshots are what's in the Screenshots known folder (`Pictures\Screenshots` or its
/// localised, moved or OneDrive-backed equivalent), where Win+PrtScn and Snipping Tool save.
#[derive(Debug, Clone)]
pub struct NativeScreenshots {
    folder: Option<PathBuf>,
}

impl NativeScreenshots {
    /// Asks the shell for the folder once. `home` is only needed on Linux; same signature
    /// everywhere.
    pub fn new(_home: PathBuf) -> Self {
        use windows_sys::Win32::UI::Shell::FOLDERID_Screenshots;
        Self {
            folder: known_folder(&FOLDERID_Screenshots),
        }
    }
}

/// Where a known folder is, without creating it when it doesn't exist (yet).
fn known_folder(id: &windows_sys::core::GUID) -> Option<PathBuf> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{KF_FLAG_DONT_VERIFY, SHGetKnownFolderPath};
    let mut raw: windows_sys::core::PWSTR = std::ptr::null_mut();
    // SAFETY: `id` is a valid GUID and `raw` a valid out-pointer; no token (the current user).
    let status = unsafe {
        SHGetKnownFolderPath(
            id,
            KF_FLAG_DONT_VERIFY as u32,
            std::ptr::null_mut(),
            &mut raw,
        )
    };
    let path = (status >= 0 && !raw.is_null()).then(|| {
        // SAFETY: on success `raw` is a NUL-terminated string the shell allocated.
        let wide = unsafe {
            let len = (0..).take_while(|&i| *raw.add(i) != 0).count();
            std::slice::from_raw_parts(raw, len)
        };
        PathBuf::from(OsString::from_wide(wide))
    });
    // SAFETY: the shell's allocation (null on some failures, which is allowed), freed once.
    unsafe { CoTaskMemFree(raw.cast()) };
    // The form Items have, where the folder exists.
    path.map(|path| resolve(&path).unwrap_or(path))
}

impl Screenshots for NativeScreenshots {
    fn is_screenshot(&self, path: &Path) -> PortResult<bool> {
        // The path alone: nothing about the file is read.
        Ok(self
            .folder
            .as_ref()
            .is_some_and(|folder| kipple_core::paths::is_within(folder, path, false)))
    }
}

/// Windows has no switch that stops reads from downloading: this only makes cloud files show
/// up as what they are. Readers must check `is_placeholder` first.
pub(crate) fn never_download_placeholders() -> PortResult<()> {
    const PHCM_EXPOSE_PLACEHOLDERS: i8 = 2;
    // SAFETY: a plain value; it only changes how this process sees cloud files.
    let previous = unsafe { RtlSetProcessPlaceholderCompatibilityMode(PHCM_EXPOSE_PLACEHOLDERS) };
    if previous < 0 {
        return Err(io::Error::other("RtlSetProcessPlaceholderCompatibilityMode failed").into());
    }
    Ok(())
}

pub(crate) fn peak_memory() -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let size = u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS>()).ok()?;
    // SAFETY: a zeroed, correctly sized struct for the current process's pseudo-handle.
    let counters = unsafe {
        let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        counters.cb = size;
        (GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) != 0).then_some(counters)
    }?;
    u64::try_from(counters.PeakWorkingSetSize).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(wide: &[u16]) -> String {
        String::from_utf16(wide.strip_suffix(&[0]).unwrap()).unwrap()
    }

    #[test]
    fn long_paths_get_the_verbatim_prefix() {
        assert_eq!(text(&verbatim(Path::new(r"C:\Users\x"))), r"\\?\C:\Users\x");
        assert_eq!(
            text(&verbatim(Path::new(r"\\nas\share\a"))),
            r"\\?\UNC\nas\share\a"
        );
        assert_eq!(text(&verbatim(Path::new(r"\\?\C:\x"))), r"\\?\C:\x");
        assert_eq!(text(&verbatim(Path::new("C:/a/b"))), r"\\?\C:\a\b");
    }

    #[test]
    fn kinds_from_attributes_and_reparse_tags() {
        const SYMLINK: u32 = 0xA000_000C;
        const JUNCTION: u32 = 0xA000_0003;
        const CLOUD: u32 = 0x9000_001A;
        let dir = FILE_ATTRIBUTE_DIRECTORY;
        let reparse = FILE_ATTRIBUTE_REPARSE_POINT;
        assert_eq!(kind_of(0, 0), EntryKind::File);
        assert_eq!(kind_of(dir, 0), EntryKind::Dir);
        assert_eq!(kind_of(reparse, SYMLINK), EntryKind::Symlink);
        assert_eq!(kind_of(dir | reparse, JUNCTION), EntryKind::Symlink);
        assert_eq!(
            kind_of(dir | reparse, CLOUD),
            EntryKind::Dir,
            "a cloud folder is a folder"
        );
        assert_eq!(kind_of(reparse, CLOUD), EntryKind::File);
    }

    #[test]
    fn filetimes_become_unix_nanoseconds() {
        assert_eq!(filetime(116_444_736_000_000_000), Timestamp(0));
        assert_eq!(filetime(116_444_736_000_000_001), Timestamp(100));
        assert_eq!(
            filetime(0),
            Timestamp(i64::MIN),
            "1601 is before what fits: it saturates"
        );
    }
}
