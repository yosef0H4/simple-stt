use anyhow::Result;
use std::time::Duration;

/// Force-terminates one known child PID and waits for the operating system to
/// report that exact process handle as signaled. This exact child PID fallback is
/// a last-resort path for
/// a disposable inference worker which ignored its framed shutdown request.
#[cfg(windows)]
pub fn force_terminate_pid(pid: u32, wait: Duration) -> Result<()> {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_TERMINATE,
    };

    const SYNCHRONIZE: u32 = 0x00100000;

    let handle = unsafe { OpenProcess(PROCESS_TERMINATE | SYNCHRONIZE, 0, pid) };
    anyhow::ensure!(
        !handle.is_null(),
        "OpenProcess failed for inference-worker pid {pid}"
    );
    let terminated = unsafe { TerminateProcess(handle, 1) };
    if terminated == 0 {
        unsafe { CloseHandle(handle) };
        anyhow::bail!("TerminateProcess failed for inference-worker pid {pid}");
    }
    let wait_ms = wait.as_millis().min(u32::MAX as u128) as u32;
    let outcome = unsafe { WaitForSingleObject(handle, wait_ms) };
    unsafe { CloseHandle(handle) };
    anyhow::ensure!(
        outcome == WAIT_OBJECT_0,
        "forced inference-worker pid {pid} did not exit within {wait_ms} ms"
    );
    Ok(())
}

#[cfg(not(windows))]
pub fn force_terminate_pid(pid: u32, _wait: Duration) -> Result<()> {
    let status = std::process::Command::new("kill")
        .arg("-9")
        .arg(pid.to_string())
        .status()
        .with_context(|| format!("launching kill for inference-worker pid {pid}"))?;
    anyhow::ensure!(
        status.success(),
        "kill -9 failed for inference-worker pid {pid}"
    );
    Ok(())
}
