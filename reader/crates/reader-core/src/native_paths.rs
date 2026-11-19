//! Library discovery for optional installed runtimes; never loads native code.
use anyhow::{Context, Result};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

pub const NVIDIA_DEPENDENCY_LIBRARY_PATHS: [&str; 3] = [
    "cuda-cudnn/nvidia/cudnn/lib",
    "cuda-cublas/nvidia/cu13/lib",
    "cuda-nvrtc/nvidia/cu13/lib",
];

const WINDOWS_NVIDIA_LIBRARY_PATHS: [&str; 4] = [
    "cuda-cudnn-windows/nvidia/cudnn/bin",
    "cuda-cublas-windows/nvidia/cu13/bin/x86_64",
    "cuda-nvrtc-windows/nvidia/cu13/bin/x86_64",
    "cuda-runtime-windows/nvidia/cu13/bin/x86_64",
];

pub fn runtime_library_directories(runtime: &Path) -> Vec<PathBuf> {
    let mut paths = vec![runtime.join("lib")];
    if cfg!(windows) {
        paths.push(runtime.join("bin"));
    }
    if runtime.file_name() == Some(OsStr::new("sherpa-cuda")) {
        if let Some(assets) = runtime.parent() {
            paths.extend(NVIDIA_DEPENDENCY_LIBRARY_PATHS.map(|path| assets.join(path)));
        }
    }
    if runtime.file_name() == Some(OsStr::new("sherpa-cuda-windows")) {
        if let Some(assets) = runtime.parent() {
            paths.extend(WINDOWS_NVIDIA_LIBRARY_PATHS.map(|path| assets.join(path)));
        }
    }
    if runtime.file_name() == Some(OsStr::new("parakeet-cuda-windows")) {
        if let Some(assets) = runtime.parent() {
            paths.push(assets.join("parakeet-cu12-runtime-windows/bin"));
            paths.push(assets.join("parakeet-cu12-cublas-windows/bin"));
        }
    }
    paths
}

pub fn configure_worker_libraries(
    command: &mut std::process::Command,
    runtime: &Path,
) -> Result<()> {
    let key = if cfg!(windows) {
        "PATH"
    } else {
        "LD_LIBRARY_PATH"
    };
    command.env(
        key,
        library_search_path(runtime, std::env::var_os(key).as_deref())?,
    );
    Ok(())
}

/// Prefer the chosen runtime and its pinned dependencies over inherited installs.
pub fn library_search_path(runtime: &Path, inherited: Option<&OsStr>) -> Result<OsString> {
    let mut paths = runtime_library_directories(runtime);
    if let Some(inherited) = inherited {
        for path in std::env::split_paths(inherited).filter(|path| !path.as_os_str().is_empty()) {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    std::env::join_paths(paths).context("Constructing native runtime library search path")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_nvidia_dependencies_precede_inherited_libraries() {
        let runtime = Path::new("assets/sherpa-cuda");
        let inherited =
            std::env::join_paths([Path::new("legacy/lib"), &runtime.join("lib")]).unwrap();
        let search = library_search_path(runtime, Some(&inherited)).unwrap();
        let paths = std::env::split_paths(&search).collect::<Vec<_>>();
        let mut expected = runtime_library_directories(runtime);
        expected.push(PathBuf::from("legacy/lib"));
        assert_eq!(paths, expected);
        assert!(paths.contains(&PathBuf::from("assets/cuda-cublas/nvidia/cu13/lib")));
        assert!(paths.contains(&PathBuf::from("assets/cuda-nvrtc/nvidia/cu13/lib")));
    }

    #[test]
    fn processor_runtime_does_not_pick_up_nvidia_dependencies_or_current_directory() {
        let runtime = Path::new("assets/sherpa-cpu");
        let search = library_search_path(runtime, Some(OsStr::new(""))).unwrap();
        assert_eq!(
            std::env::split_paths(&search).collect::<Vec<_>>(),
            runtime_library_directories(runtime)
        );
    }
}
