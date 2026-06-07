use anyhow::Result;
use std::process::Command;
use std::time::Duration;

#[cfg(windows)]
fn get_process_ram_kb() -> Result<u64> {
    use std::mem::size_of;
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    unsafe {
        let mut counters = PROCESS_MEMORY_COUNTERS {
            cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            PageFaultCount: 0,
            PeakWorkingSetSize: 0,
            WorkingSetSize: 0,
            QuotaPeakPagedPoolUsage: 0,
            QuotaPagedPoolUsage: 0,
            QuotaPeakNonPagedPoolUsage: 0,
            QuotaNonPagedPoolUsage: 0,
            PagefileUsage: 0,
            PeakPagefileUsage: 0,
        };
        if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size_of::<PROCESS_MEMORY_COUNTERS>() as u32) == 0 {
            return Ok(0);
        }
        Ok((counters.WorkingSetSize / 1024) as u64)
    }
}

#[cfg(not(windows))]
fn get_process_ram_kb() -> Result<u64> {
    Ok(0)
}

fn get_process_vram_mb(pid: u32) -> Result<u64> {
    let out = match Command::new("nvidia-smi")
        .args([
            "--query-compute-apps=pid,used_gpu_memory",
            "--format=csv,noheader,nounits",
        ])
        .output()
    {
        Ok(o) => o,
        Err(_) => return Ok(0),
    };
    if !out.status.success() {
        return Ok(0);
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let sum: u64 = s
        .lines()
        .filter_map(|line| {
            let mut parts = line.split(',');
            let app_pid = parts.next()?.trim().parse::<u32>().ok()?;
            let mem = parts.next()?.trim().parse::<u64>().ok()?;
            if app_pid == pid {
                Some(mem)
            } else {
                None
            }
        })
        .sum();
    Ok(sum)
}

fn get_current_proc_vram_mb() -> Result<u64> {
    get_process_vram_mb(std::process::id())
}

fn get_global_vram_mb() -> Result<u64> {
    let out = match Command::new("nvidia-smi")
        .args(["--query-gpu=memory.used", "--format=csv,noheader,nounits"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return Ok(0),
    };
    if !out.status.success() {
        return Ok(0);
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let sum: u64 = s
        .lines()
        .filter_map(|line| line.trim().parse::<u64>().ok())
        .sum();
    Ok(sum)
}

#[test]
#[cfg(windows)]
fn idle_unload_integration() -> Result<()> {
    // Load config and validate presence of runtime/model files.
    let config = uvox::config::AppConfig::load()?;
    config.validate()?;
    config.validate_parakeet_files()?;

    let pid = std::process::id();
    println!("collecting baseline memory/VRAM for PID {}", pid);
    let before_ram = get_process_ram_kb()?;
    let before_proc_vram = get_current_proc_vram_mb()?;
    let before_global_vram = get_global_vram_mb()?;
    println!(
        "before: proc_ram_kb={} proc_vram_mb={} global_vram_mb={}",
        before_ram, before_proc_vram, before_global_vram
    );

    println!("loading Parakeet context from config");
    let mut engine = uvox::parakeet_native::ParakeetNative::load_from_config(&config)?;
    assert!(engine.is_context_loaded());

    let loaded_ram = get_process_ram_kb()?;
    let loaded_proc_vram = get_current_proc_vram_mb()?;
    let loaded_global_vram = get_global_vram_mb()?;
    println!(
        "loaded: proc_ram_kb={} proc_vram_mb={} global_vram_mb={}",
        loaded_ram, loaded_proc_vram, loaded_global_vram
    );

    println!("waiting 30s while model is loaded...");
    std::thread::sleep(Duration::from_secs(30));

    println!("unloading Parakeet context");
    engine.unload_context();
    assert!(!engine.is_context_loaded());

    // Give OS a moment to reclaim memory
    std::thread::sleep(Duration::from_secs(2));

    let after_ram = get_process_ram_kb()?;
    let after_proc_vram = get_current_proc_vram_mb()?;
    let after_global_vram = get_global_vram_mb()?;
    println!(
        "after: proc_ram_kb={} proc_vram_mb={} global_vram_mb={}",
        after_ram, after_proc_vram, after_global_vram
    );

    let global_diff = if before_global_vram > after_global_vram {
        before_global_vram - after_global_vram
    } else {
        after_global_vram - before_global_vram
    };
    let allowed_global_diff = 128;
    assert!(
        global_diff <= allowed_global_diff,
        "global VRAM did not return to near-baseline after unload (diff {} MiB)",
        global_diff
    );

    println!(
        "measurements summary:\n  before: proc_ram_kb={} proc_vram_mb={} global_vram_mb={}\n  loaded: proc_ram_kb={} proc_vram_mb={} global_vram_mb={}\n  after:  proc_ram_kb={} proc_vram_mb={} global_vram_mb={}",
        before_ram,
        before_proc_vram,
        before_global_vram,
        loaded_ram,
        loaded_proc_vram,
        loaded_global_vram,
        after_ram,
        after_proc_vram,
        after_global_vram
    );

    Ok(())
}
