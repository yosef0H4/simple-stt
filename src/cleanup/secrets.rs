use anyhow::{Context, Result};
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};

const SERVICE: &str = "simple-stt";
const COMPATIBLE_KEY_ACCOUNT: &str = "cleanup-compatible-api-key";
const CHATGPT_ACCOUNT: &str = "cleanup-chatgpt-oauth";

pub fn compatible_api_key() -> Result<Option<String>> {
    if let Ok(value) = std::env::var("SIMPLE_STT_AI_API_KEY") {
        if !value.trim().is_empty() {
            return Ok(Some(value));
        }
    }
    get(COMPATIBLE_KEY_ACCOUNT)
}

pub fn set_compatible_api_key(value: &str) -> Result<()> {
    anyhow::ensure!(!value.trim().is_empty(), "API key must not be empty");
    set(COMPATIBLE_KEY_ACCOUNT, value.trim())
}

pub fn delete_compatible_api_key() -> Result<()> {
    delete(COMPATIBLE_KEY_ACCOUNT)
}

pub(crate) fn chatgpt_tokens() -> Result<Option<String>> {
    get(CHATGPT_ACCOUNT)
}

pub(crate) fn set_chatgpt_tokens(value: &str) -> Result<()> {
    set(CHATGPT_ACCOUNT, value)
}

pub fn delete_chatgpt_tokens() -> Result<()> {
    delete(CHATGPT_ACCOUNT)
}

#[cfg(windows)]
fn get(account: &str) -> Result<Option<String>> {
    use std::ptr;
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND};
    use windows_sys::Win32::Security::Credentials::{
        CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC,
    };
    let target = wide_target(account);
    let mut credential: *mut CREDENTIALW = ptr::null_mut();
    let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) };
    if ok == 0 {
        let error = unsafe { GetLastError() };
        if error == ERROR_NOT_FOUND {
            return Ok(None);
        }
        anyhow::bail!("Windows Credential Manager read failed ({error})");
    }
    let value = unsafe {
        let credential_ref = &*credential;
        let bytes = std::slice::from_raw_parts(
            credential_ref.CredentialBlob,
            credential_ref.CredentialBlobSize as usize,
        );
        let result = String::from_utf8(bytes.to_vec()).context("credential is not valid UTF-8");
        CredFree(credential.cast());
        result?
    };
    Ok(Some(value))
}

#[cfg(windows)]
fn set(account: &str, secret: &str) -> Result<()> {
    use std::ptr;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Security::Credentials::{
        CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };
    let mut target = wide_target(account);
    let mut user = account
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let bytes = secret.as_bytes();
    let credential = CREDENTIALW {
        Flags: 0,
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_mut_ptr(),
        Comment: ptr::null_mut(),
        LastWritten: Default::default(),
        CredentialBlobSize: bytes.len().try_into().context("credential is too large")?,
        CredentialBlob: bytes.as_ptr().cast_mut(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: ptr::null_mut(),
        TargetAlias: ptr::null_mut(),
        UserName: user.as_mut_ptr(),
    };
    let ok = unsafe { CredWriteW(&credential, 0) };
    anyhow::ensure!(
        ok != 0,
        "Windows Credential Manager write failed ({})",
        unsafe { GetLastError() }
    );
    Ok(())
}

#[cfg(windows)]
fn delete(account: &str) -> Result<()> {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND};
    use windows_sys::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};
    let target = wide_target(account);
    let ok = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
    if ok == 0 {
        let error = unsafe { GetLastError() };
        anyhow::ensure!(
            error == ERROR_NOT_FOUND,
            "Windows Credential Manager delete failed ({error})"
        );
    }
    Ok(())
}

#[cfg(windows)]
fn wide_target(account: &str) -> Vec<u16> {
    format!("{SERVICE}/{account}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(target_os = "linux")]
fn get(account: &str) -> Result<Option<String>> {
    let output = Command::new("secret-tool")
        .args(["lookup", "service", SERVICE, "account", account])
        .output()
        .context("reading the desktop secret store (install libsecret-tools if secret-tool is unavailable)")?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8(output.stdout).context("secret store returned invalid UTF-8")?;
    let value = value.trim_end_matches(['\r', '\n']).to_owned();
    Ok((!value.is_empty()).then_some(value))
}

#[cfg(target_os = "linux")]
fn set(account: &str, secret: &str) -> Result<()> {
    use std::io::Write;
    let mut child = Command::new("secret-tool")
        .args(["store", "--label", "Simple STT AI cleanup", "service", SERVICE, "account", account])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .context("opening the desktop secret store (install libsecret-tools if secret-tool is unavailable)")?;
    child
        .stdin
        .take()
        .context("secret-tool input was unavailable")?
        .write_all(secret.as_bytes())?;
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
        .status()
        .context("opening the desktop secret store")?;
    anyhow::ensure!(
        status.success(),
        "desktop secret store could not delete the credential"
    );
    Ok(())
}

#[cfg(not(any(windows, target_os = "linux")))]
fn get(_account: &str) -> Result<Option<String>> {
    anyhow::bail!("secure AI credentials are not supported on this platform")
}

#[cfg(not(any(windows, target_os = "linux")))]
fn set(_account: &str, _secret: &str) -> Result<()> {
    anyhow::bail!("secure AI credentials are not supported on this platform")
}

#[cfg(not(any(windows, target_os = "linux")))]
fn delete(_account: &str) -> Result<()> {
    anyhow::bail!("secure AI credentials are not supported on this platform")
}
