// SPDX-License-Identifier: GPL-3.0-or-later
//! Paths as BLOBs, losslessly: the raw bytes on Unix (names needn't be UTF-8), UTF-16 code units
//! on Windows (names needn't be valid UTF-16). The index is local, so the encoding never has to
//! travel between OSes.

use std::path::{Path, PathBuf};

#[cfg(unix)]
pub fn encode(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
pub fn decode(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::OsStr::from_bytes(bytes).into()
}

#[cfg(windows)]
pub fn encode(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(windows)]
pub fn decode(bytes: &[u8]) -> PathBuf {
    use std::os::windows::ffi::OsStringExt;
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    std::ffi::OsString::from_wide(&units).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for path in [
            "/Users/zoë/Belastingen 2025/aangifte.pdf",
            r"C:\Users\Zoë\x.txt",
            "",
            "日本語/写真.jpg",
        ] {
            assert_eq!(decode(&encode(Path::new(path))), Path::new(path));
        }
    }

    #[cfg(unix)]
    #[test]
    fn keeps_names_that_are_not_utf8() {
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(std::ffi::OsStr::from_bytes(b"/mnt/usb/caf\xe9.txt"));
        assert_eq!(decode(&encode(path)), path);
    }
}
