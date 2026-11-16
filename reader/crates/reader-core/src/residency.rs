//! One-shot resource estimates for idle disposable workers.
use serde::Serialize;

pub const RAM_LIMIT_BYTES: u64 = 1024 * 1024 * 1024;
pub const VRAM_LIMIT_BYTES: u64 = 768 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Cpu,
    Gpu,
    Unknown,
}

#[derive(Clone, Debug, Serialize)]
pub struct Estimate {
    pub ram_bytes: Option<u64>,
    pub ram_source: &'static str,
    pub vram_bytes: Option<u64>,
    pub vram_source: &'static str,
    pub reason: &'static str,
}

impl Estimate {
    pub fn within_budget(&self) -> bool {
        // Diagnostic classification only. Worker lifetime is governed by idle
        // timeouts and actual system memory pressure, never these estimates.
        self.ram_bytes.is_none_or(|v| v <= RAM_LIMIT_BYTES)
            && self.vram_bytes.is_none_or(|v| v <= VRAM_LIMIT_BYTES)
    }
}

pub fn measure(pid: u32, device: Device) -> Estimate {
    let (ram_bytes, ram_source) = ram(pid);
    if ram_bytes.is_some_and(|v| v > RAM_LIMIT_BYTES) {
        return Estimate {
            ram_bytes,
            ram_source,
            vram_bytes: None,
            vram_source: "not_sampled",
            reason: "ram_over_budget",
        };
    }
    let (vram_bytes, vram_source) = match device {
        Device::Cpu => (Some(0), "cpu"),
        Device::Gpu => gpu(pid).map_or((None, "unknown"), |v| (Some(v), "nvidia_smi")),
        Device::Unknown => (None, "unknown"),
    };
    let reason = match vram_bytes {
        Some(value) if value > VRAM_LIMIT_BYTES => "vram_over_budget",
        _ if ram_bytes.is_none() => "ram_unknown",
        None => "vram_unknown",
        Some(_) => "within_budget",
    };
    Estimate {
        ram_bytes,
        ram_source,
        vram_bytes,
        vram_source,
        reason,
    }
}

#[cfg(target_os = "linux")]
fn ram(pid: u32) -> (Option<u64>, &'static str) {
    fn field(path: String, name: &str) -> Option<u64> {
        let text = std::fs::read_to_string(path).ok()?;
        text.lines().find_map(|line| {
            line.strip_prefix(name)?
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()?
                .checked_mul(1024)
        })
    }
    if let Some(value) = field(format!("/proc/{pid}/smaps_rollup"), "Pss:") {
        (Some(value), "pss")
    } else if let Some(value) = field(format!("/proc/{pid}/status"), "VmRSS:") {
        (Some(value), "rss")
    } else {
        (None, "unknown")
    }
}

#[cfg(windows)]
fn ram(pid: u32) -> (Option<u64>, &'static str) {
    use std::mem::size_of;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    // A query-only handle keeps the probe scoped to this child PID. An EX
    // counter reports private committed bytes, not Linux proportional RSS.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return (None, "unknown");
    }
    let mut extended = PROCESS_MEMORY_COUNTERS_EX {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    // The Win32 API accepts the common counter prefix and fills the extended
    // fields when cb specifies the size of PROCESS_MEMORY_COUNTERS_EX.
    let ok = unsafe {
        K32GetProcessMemoryInfo(
            handle,
            (&mut extended as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
            size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        )
    };
    let result = if ok != 0 {
        (Some(extended.PrivateUsage as u64), "private_commit")
    } else {
        let mut basic = PROCESS_MEMORY_COUNTERS {
            cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            ..Default::default()
        };
        let ok = unsafe {
            K32GetProcessMemoryInfo(
                handle,
                &mut basic,
                size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            )
        };
        if ok != 0 {
            (Some(basic.WorkingSetSize as u64), "working_set")
        } else {
            (None, "unknown")
        }
    };
    unsafe { CloseHandle(handle) };
    result
}

#[cfg(not(any(target_os = "linux", windows)))]
fn ram(_pid: u32) -> (Option<u64>, &'static str) {
    (None, "unknown")
}

#[cfg(target_os = "linux")]
fn gpu(pid: u32) -> Option<u64> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let mut child = Command::new("nvidia-smi")
        .args([
            "--query-compute-apps=pid,used_gpu_memory",
            "--format=csv,noheader,nounits",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_millis(300);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return None,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut output = String::new();
    child
        .stdout
        .take()?
        .take(65536)
        .read_to_string(&mut output)
        .ok()?;
    output.lines().find_map(|line| {
        let (found, mib) = line.split_once(',')?;
        if found.trim().parse::<u32>().ok()? != pid {
            return None;
        }
        mib.trim().parse::<u64>().ok()?.checked_mul(1024 * 1024)
    })
}

#[cfg(not(target_os = "linux"))]
fn gpu(_pid: u32) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    fn estimate(ram: Option<u64>, vram: Option<u64>) -> Estimate {
        Estimate {
            ram_bytes: ram,
            ram_source: "fixture",
            vram_bytes: vram,
            vram_source: "fixture",
            reason: "fixture",
        }
    }
    #[cfg(windows)]
    #[test]
    fn current_process_memory_can_be_queried() {
        let (bytes, source) = ram(std::process::id());
        assert!(bytes.is_some_and(|value| value > 0));
        assert!(matches!(source, "private_commit" | "working_set"));
        assert_eq!(ram(0), (None, "unknown"));
    }
    #[test]
    fn diagnostics_distinguish_small_and_large_workers() {
        const MIB: u64 = 1024 * 1024;
        // Measured Vulkan and NVIDIA speech workers after transcription.
        assert!(estimate(Some(317 * MIB), Some(408 * MIB)).within_budget());
        assert!(estimate(Some(572 * MIB), Some(526 * MIB)).within_budget());
        // An unknown Vulkan VRAM reading must not force immediate eviction.
        assert!(estimate(Some(317 * MIB), None).within_budget());
        // Measured larger GPU voice worker exceeds both budgets.
        assert!(!estimate(Some(1350 * MIB), Some(1170 * MIB)).within_budget());
    }

    #[test]
    fn diagnostics_require_known_measurements_to_report_excess() {
        assert!(estimate(Some(RAM_LIMIT_BYTES), Some(VRAM_LIMIT_BYTES)).within_budget());
        assert!(!estimate(Some(RAM_LIMIT_BYTES + 1), Some(0)).within_budget());
        assert!(!estimate(Some(1), Some(VRAM_LIMIT_BYTES + 1)).within_budget());
        assert!(estimate(None, Some(0)).within_budget());
        assert!(estimate(Some(1), None).within_budget());
        assert!(!estimate(None, Some(VRAM_LIMIT_BYTES + 1)).within_budget());
        assert!(!estimate(Some(RAM_LIMIT_BYTES + 1), None).within_budget());
    }
}
