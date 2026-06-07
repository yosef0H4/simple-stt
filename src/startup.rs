use anyhow::{Context, Result};
use std::ptr::null_mut;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_SET_VALUE, REG_SZ,
};

use crate::winutil::wide_null;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "Uvox";

pub fn set_start_with_windows(enabled: bool) -> Result<()> {
    let mut key: HKEY = null_mut();
    let run_key = wide_null(RUN_KEY);
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            run_key.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut key,
        )
    };
    anyhow::ensure!(
        status == 0,
        "opening Windows Run registry key failed: {status}"
    );
    let result = if enabled {
        let exe = std::env::current_exe().context("resolving current executable")?;
        let command = format!("\"{}\" run", exe.display());
        let command = wide_null(&command);
        let name = wide_null(VALUE_NAME);
        let bytes = command.len() * std::mem::size_of::<u16>();
        let status = unsafe {
            RegSetValueExW(
                key,
                name.as_ptr(),
                0,
                REG_SZ,
                command.as_ptr().cast(),
                bytes as u32,
            )
        };
        anyhow::ensure!(
            status == 0,
            "writing startup registry value failed: {status}"
        );
        Ok(())
    } else {
        let name = wide_null(VALUE_NAME);
        let status = unsafe { RegDeleteValueW(key, name.as_ptr()) };
        if status == 0 || status == 2 {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "deleting startup registry value failed: {status}"
            ))
        }
    };
    unsafe { RegCloseKey(key) };
    result
}
