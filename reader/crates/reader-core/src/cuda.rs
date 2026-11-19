//! Independent verification used by the TTS and OCR workers after inference.

pub fn onnx_cuda_verified() -> bool {
    #[cfg(any(target_os = "linux", windows))]
    {
        provider_loaded() && nvidia_reports_worker()
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    false
}

#[cfg(target_os = "linux")]
fn provider_loaded() -> bool {
    std::fs::read_to_string("/proc/self/maps")
        .is_ok_and(|maps| maps.contains("libonnxruntime_providers_cuda"))
}

#[cfg(windows)]
fn provider_loaded() -> bool {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    !unsafe { GetModuleHandleW(windows_sys::w!("onnxruntime_providers_cuda.dll")) }.is_null()
}

#[cfg(any(target_os = "linux", windows))]
fn nvidia_reports_worker() -> bool {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let mut command = Command::new("nvidia-smi");
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut command, 0x0800_0000);
    let Ok(mut child) = command
        .args([
            "--query-compute-apps=pid,used_gpu_memory",
            "--format=csv,noheader,nounits",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
    child.wait_with_output().is_ok_and(|output| {
        reports_worker(
            &String::from_utf8_lossy(&output.stdout),
            std::process::id(),
            cfg!(windows),
        )
    })
}

#[cfg(any(target_os = "linux", windows, test))]
fn reports_worker(output: &str, pid: u32, wddm: bool) -> bool {
    output.lines().any(|line| {
        let Some((process, memory)) = line.split_once(',') else {
            return false;
        };
        let memory = memory.trim();
        // WDDM tracks allocations itself. NVIDIA reports the process but cannot
        // report its memory size, so require the PID and the loaded CUDA provider.
        process.trim().parse::<u32>() == Ok(pid)
            && (memory.parse::<u64>().is_ok_and(|size| size > 0)
                || (wddm && matches!(memory, "[N/A]" | "N/A")))
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_the_inference_process_can_prove_cuda() {
        let output = "42, 248\n53, [N/A]\n57, 0\n";
        assert!(super::reports_worker(output, 42, false));
        assert!(super::reports_worker(output, 53, true));
        assert!(!super::reports_worker(output, 53, false));
        assert!(!super::reports_worker(output, 57, true));
        assert!(!super::reports_worker(output, 99, true));
    }
}
