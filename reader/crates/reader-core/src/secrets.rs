use anyhow::{Context, Result};
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};

const SERVICE: &str = "simple-stt-reader";
pub fn speech_key() -> Result<Option<String>> {
    if let Ok(value) = std::env::var("SIMPLE_TTS_SPEECH_API_KEY") {
        if !value.trim().is_empty() {
            return Ok(Some(value));
        }
    }
    get("speech-api-key")
}
pub fn ocr_key() -> Result<Option<String>> {
    if let Ok(value) = std::env::var("SIMPLE_TTS_OCR_API_KEY") {
        if !value.trim().is_empty() {
            return Ok(Some(value));
        }
    }
    get("ocr-api-key")
}
pub fn set_speech_key(value: &str) -> Result<()> {
    set("speech-api-key", value)
}
pub fn set_ocr_key(value: &str) -> Result<()> {
    set("ocr-api-key", value)
}
pub fn delete_speech_key() -> Result<()> {
    delete("speech-api-key")
}
pub fn delete_ocr_key() -> Result<()> {
    delete("ocr-api-key")
}
pub fn server_key() -> Result<Option<String>> {
    get("server-auth-token")
}
pub fn set_server_key(value: &str) -> Result<()> {
    set("server-auth-token", value)
}

#[cfg(windows)]
fn target(account: &str) -> Vec<u16> {
    format!("{SERVICE}/{account}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}
#[cfg(windows)]
fn get(account: &str) -> Result<Option<String>> {
    use std::ptr;
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND};
    use windows_sys::Win32::Security::Credentials::{
        CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC,
    };
    let name = target(account);
    let mut entry: *mut CREDENTIALW = ptr::null_mut();
    if unsafe { CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut entry) } == 0 {
        let error = unsafe { GetLastError() };
        if error == ERROR_NOT_FOUND {
            return Ok(None);
        };
        anyhow::bail!("Windows Credential Manager read failed ({error})")
    }
    let result = unsafe {
        let credential = &*entry;
        let bytes = std::slice::from_raw_parts(
            credential.CredentialBlob,
            credential.CredentialBlobSize as usize,
        );
        let text = String::from_utf8(bytes.to_vec()).context("stored credential is not UTF-8");
        CredFree(entry.cast());
        text?
    };
    Ok(Some(result))
}
#[cfg(windows)]
fn set(account: &str, value: &str) -> Result<()> {
    use std::ptr;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Security::Credentials::{
        CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };
    anyhow::ensure!(!value.trim().is_empty(), "API key cannot be empty");
    let mut name = target(account);
    let mut user = account
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let bytes = value.trim().as_bytes();
    let entry = CREDENTIALW {
        Flags: 0,
        Type: CRED_TYPE_GENERIC,
        TargetName: name.as_mut_ptr(),
        Comment: ptr::null_mut(),
        LastWritten: Default::default(),
        CredentialBlobSize: bytes.len().try_into().context("API key is too long")?,
        CredentialBlob: bytes.as_ptr().cast_mut(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: ptr::null_mut(),
        TargetAlias: ptr::null_mut(),
        UserName: user.as_mut_ptr(),
    };
    anyhow::ensure!(
        unsafe { CredWriteW(&entry, 0) } != 0,
        "Windows Credential Manager write failed ({})",
        unsafe { GetLastError() }
    );
    Ok(())
}
#[cfg(windows)]
fn delete(account: &str) -> Result<()> {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND};
    use windows_sys::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};
    let name = target(account);
    if unsafe { CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
        let error = unsafe { GetLastError() };
        anyhow::ensure!(
            error == ERROR_NOT_FOUND,
            "Windows Credential Manager delete failed ({error})"
        )
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn get(account: &str) -> Result<Option<String>> {
    let output=Command::new("secret-tool").args(["lookup","service",SERVICE,"account",account]).output().context("reading GNOME Keyring/KDE Wallet (install libsecret-tools if secret-tool is unavailable)")?;
    if !output.status.success() {
        return Ok(None);
    };
    let value = String::from_utf8(output.stdout).context("secret store returned invalid UTF-8")?;
    let value = value.trim_end_matches(['\r', '\n']).to_owned();
    Ok((!value.is_empty()).then_some(value))
}
#[cfg(target_os = "linux")]
fn set(account: &str, value: &str) -> Result<()> {
    use std::io::Write;
    anyhow::ensure!(!value.trim().is_empty(), "API key cannot be empty");
    let mut child = Command::new("secret-tool")
        .args([
            "store",
            "--label",
            "Simple TTS provider credential",
            "service",
            SERVICE,
            "account",
            account,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("opening GNOME Keyring/KDE Wallet (install libsecret-tools if unavailable)")?;
    child
        .stdin
        .take()
        .context("secret store input unavailable")?
        .write_all(value.trim().as_bytes())?;
    let status = child.wait()?;
    anyhow::ensure!(
        status.success(),
        "desktop secret store rejected the credential"
    );
    Ok(())
}
#[cfg(target_os = "linux")]
fn delete(account: &str) -> Result<()> {
    let status = Command::new("secret-tool")
        .args(["clear", "service", SERVICE, "account", account])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("opening desktop secret store")?;
    anyhow::ensure!(
        status.success(),
        "desktop secret store could not delete the credential"
    );
    Ok(())
}

#[cfg(not(any(windows, target_os = "linux")))]
fn get(_: &str) -> Result<Option<String>> {
    anyhow::bail!("secure provider credentials are unsupported on this platform")
}
#[cfg(not(any(windows, target_os = "linux")))]
fn set(_: &str, _: &str) -> Result<()> {
    anyhow::bail!("secure provider credentials are unsupported on this platform")
}
#[cfg(not(any(windows, target_os = "linux")))]
fn delete(_: &str) -> Result<()> {
    anyhow::bail!("secure provider credentials are unsupported on this platform")
}
