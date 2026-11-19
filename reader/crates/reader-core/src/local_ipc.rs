//! Reader IPC addresses and connections shared by the host, CLI and daemon.

use std::path::Path;

#[cfg(unix)]
pub fn address(profile: &Path) -> std::path::PathBuf {
    profile.join("simple-tts.sock")
}

#[cfg(unix)]
pub fn connect(profile: &Path) -> std::io::Result<std::os::unix::net::UnixStream> {
    std::os::unix::net::UnixStream::connect(address(profile))
}

#[cfg(windows)]
pub fn address(profile: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    // Resolve junctions and aliases when the profile exists. Both endpoints
    // compute this after creating the profile, and callers checking for an
    // absent profile can use its absolute path without creating anything.
    let path = profile
        .canonicalize()
        .or_else(|_| std::path::absolute(profile))?;
    let path = path.as_os_str().to_string_lossy().replace('/', "\\");
    let path = if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(&path).to_owned()
    };
    let hash = Sha256::digest(path.trim_end_matches('\\').to_lowercase().as_bytes());
    Ok(format!(r"\\.\pipe\simple-stt-reader-{hash:x}"))
}

#[cfg(windows)]
pub fn wide_address(profile: &Path) -> std::io::Result<Vec<u16>> {
    Ok(address(profile)?.encode_utf16().chain(Some(0)).collect())
}

#[cfg(windows)]
pub fn connect(profile: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;
    use windows_sys::Win32::System::Pipes::WaitNamedPipeW;
    let path = address(profile)?;
    let open = || {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&path)
    };
    match open() {
        Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
            let wide = path.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
            if unsafe { WaitNamedPipeW(wide.as_ptr(), 500) } == 0 {
                return Err(std::io::Error::last_os_error());
            }
            open()
        }
        result => result,
    }
}

/// Call before spawning children. Command supplies the child's own stdio,
/// but Windows can also inherit the original caller's redirected handles.
/// A resident daemon holding those handles keeps the caller waiting for EOF.
#[cfg(windows)]
pub fn prevent_stdio_handle_inheritance() {
    use windows_sys::Win32::Foundation::{
        SetHandleInformation, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    for id in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        let handle = unsafe { GetStdHandle(id) };
        if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
            unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) };
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn pipe_address_is_scoped_to_the_reader_profile() {
        let profile = tempfile::tempdir().unwrap();
        assert_eq!(
            address(profile.path()).unwrap(),
            address(&profile.path().join(".")).unwrap()
        );
        assert_ne!(
            address(profile.path()).unwrap(),
            address(&profile.path().join("other-profile")).unwrap()
        );
    }
}
#[cfg(windows)]
pub const WINDOWS_PICKER_CLASS: *const u16 = windows_sys::w!("SimpleSttScreenRegionPicker");
#[cfg(windows)]
pub const WINDOWS_TOOLTIP_PROPERTY: *const u16 = windows_sys::w!("SimpleSttOverlay");
