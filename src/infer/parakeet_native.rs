use anyhow::{anyhow, Context, Result};
use libloading::Library;
use std::ffi::{c_char, c_float, c_int, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;

#[cfg(windows)]
use windows_sys::Win32::System::LibraryLoader::{
    AddDllDirectory, SetDefaultDllDirectories, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
    LOAD_LIBRARY_SEARCH_USER_DIRS,
};

type Ctx = *mut c_void;

pub struct ParakeetNative {
    api: Api,
    ctx: Ctx,
}

impl ParakeetNative {
    pub fn load(runtime_dir: &Path, model_path: &Path) -> Result<Self> {
        let api = Api::load(runtime_dir)?;
        let ctx = create_context(&api, model_path)?;
        Ok(Self { api, ctx })
    }

    pub fn transcribe_wav(&self, path: &Path) -> Result<String> {
        anyhow::ensure!(path.exists(), "audio file is missing: {}", path.display());
        let path = CString::new(path.to_string_lossy().as_bytes())
            .context("audio path contains an interior NUL byte")?;
        let ptr = unsafe { (self.api.transcribe_path)(self.ctx, path.as_ptr(), 0) };
        self.take_string(ptr, "parakeet_capi_transcribe_path")
    }

    pub fn transcribe_pcm16_16k(&self, samples: &[i16]) -> Result<String> {
        let pcm: Vec<f32> = samples
            .iter()
            .map(|sample| *sample as f32 / 32768.0)
            .collect();
        let ptr = unsafe {
            (self.api.transcribe_pcm)(self.ctx, pcm.as_ptr(), pcm.len() as c_int, 16_000, 0)
        };
        self.take_string(ptr, "parakeet_capi_transcribe_pcm")
    }

    fn take_string(&self, ptr: *mut c_char, operation: &str) -> Result<String> {
        if ptr.is_null() {
            let error = unsafe { CStr::from_ptr((self.api.last_error)(self.ctx)) }
                .to_string_lossy()
                .into_owned();
            if error.trim().is_empty() {
                return Err(anyhow!("{operation} failed"));
            }
            return Err(anyhow!("{operation} failed: {error}"));
        }
        let text = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();
        unsafe { (self.api.free_string)(ptr) };
        Ok(text.trim().to_owned())
    }
}

impl Drop for ParakeetNative {
    fn drop(&mut self) {
        if !self.ctx.is_null() {
            unsafe { (self.api.free)(self.ctx) };
            self.ctx = null_mut();
        }
    }
}

struct Api {
    _lib: Library,
    load: unsafe extern "C" fn(*const c_char) -> Ctx,
    free: unsafe extern "C" fn(Ctx),
    transcribe_path: unsafe extern "C" fn(Ctx, *const c_char, c_int) -> *mut c_char,
    transcribe_pcm: unsafe extern "C" fn(Ctx, *const c_float, c_int, c_int, c_int) -> *mut c_char,
    free_string: unsafe extern "C" fn(*mut c_char),
    last_error: unsafe extern "C" fn(Ctx) -> *const c_char,
}

impl Api {
    fn load(runtime_dir: &Path) -> Result<Self> {
        configure_native_search(runtime_dir)?;
        let library_path = parakeet_library_path(runtime_dir)?;
        let lib = unsafe { Library::new(&library_path) }
            .with_context(|| format!("loading {}", library_path.display()))?;
        unsafe {
            let abi_version: unsafe extern "C" fn() -> c_int =
                sym(&lib, b"parakeet_capi_abi_version\0")?;
            let actual_abi = abi_version();
            anyhow::ensure!(
                actual_abi == 6,
                "unsupported parakeet.cpp C ABI {actual_abi}; expected 6 (v0.5.0)"
            );
            #[cfg(target_os = "linux")]
            select_vulkan_device(&lib)?;
            #[cfg(windows)]
            select_windows_vulkan_device()?;
            Ok(Self {
                load: sym(&lib, b"parakeet_capi_load\0")?,
                free: sym(&lib, b"parakeet_capi_free\0")?,
                transcribe_path: sym(&lib, b"parakeet_capi_transcribe_path\0")?,
                transcribe_pcm: sym(&lib, b"parakeet_capi_transcribe_pcm\0")?,
                free_string: sym(&lib, b"parakeet_capi_free_string\0")?,
                last_error: sym(&lib, b"parakeet_capi_last_error\0")?,
                _lib: lib,
            })
        }
    }
}

#[cfg(windows)]
unsafe fn select_windows_vulkan_device() -> Result<()> {
    let requested = std::env::var("PARAKEET_DEVICE").ok();
    if requested.as_deref() == Some("cpu") {
        set_windows_native_env("PARAKEET_DEVICE", "cpu")?;
        return Ok(());
    }
    // The Windows runtime only exports the Parakeet C API, not ggml's
    // enumeration API. Query the same Vulkan loader before ggml initializes.
    let result = windows_vulkan_devices();
    match result {
        Ok(devices) => {
            if let Some(index) = preferred_vulkan_device(&devices) {
                let (_, description, _) = &devices[index];
                // ggml applies this filter to Vulkan physical-device indices,
                // then exposes the sole selected device as Vulkan0.
                set_windows_native_env("GGML_VK_VISIBLE_DEVICES", &index.to_string())?;
                set_windows_native_env("PARAKEET_DEVICE", "Vulkan0")?;
                tracing::info!(physical_index = index, gpu = %description, "selected physical Vulkan GPU");
                return Ok(());
            }
            anyhow::ensure!(
                requested.is_none(),
                "GPU mode requires a physical Vulkan GPU; no GPU was found"
            );
        }
        Err(error) => {
            if requested.is_some() {
                return Err(error).context("GPU mode requires an available Vulkan GPU");
            }
            tracing::warn!(%error, "Vulkan unavailable; automatic mode uses CPU");
        }
    }
    set_windows_native_env("PARAKEET_DEVICE", "cpu")?;
    Ok(())
}

#[cfg(windows)]
unsafe fn set_windows_native_env(key: &str, value: &str) -> Result<()> {
    // Rust updates the Win32 environment; the MSVC DLL reads UCRT's getenv.
    // Keep both views synchronized even if UCRT initialized before DLL load.
    let crt = Library::new("ucrtbase.dll").context("loading native runtime environment API")?;
    let putenv: unsafe extern "C" fn(*const c_char, *const c_char) -> c_int =
        sym(&crt, b"_putenv_s\0")?;
    let key_c = CString::new(key)?;
    let value_c = CString::new(value)?;
    anyhow::ensure!(
        putenv(key_c.as_ptr(), value_c.as_ptr()) == 0,
        "failed to update native runtime environment variable {key}"
    );
    std::env::set_var(key, value);
    Ok(())
}

#[cfg(windows)]
unsafe fn windows_vulkan_devices() -> Result<Vec<(String, String, c_int)>> {
    use ash::{vk, Entry};
    let entry = Entry::load().context("loading Vulkan driver")?;
    let application = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_2);
    let info = vk::InstanceCreateInfo::default().application_info(&application);
    let instance = entry
        .create_instance(&info, None)
        .map_err(|error| anyhow!("creating Vulkan instance: {error:?}"))?;
    let result = (|| {
        let physical_devices = instance
            .enumerate_physical_devices()
            .map_err(|error| anyhow!("enumerating Vulkan devices: {error:?}"))?;
        Ok(physical_devices
            .iter()
            .enumerate()
            .map(|(index, device)| {
                let properties = instance.get_physical_device_properties(*device);
                let description = CStr::from_ptr(properties.device_name.as_ptr())
                    .to_string_lossy()
                    .into_owned();
                let compute = instance
                    .get_physical_device_queue_family_properties(*device)
                    .iter()
                    .any(|queue| queue.queue_flags.contains(vk::QueueFlags::COMPUTE));
                let mut vulkan11 = vk::PhysicalDeviceVulkan11Features::default();
                let mut features = vk::PhysicalDeviceFeatures2::default().push_next(&mut vulkan11);
                instance.get_physical_device_features2(*device, &mut features);
                let kind = if !compute
                    || properties.api_version < vk::API_VERSION_1_2
                    || vulkan11.storage_buffer16_bit_access == vk::FALSE
                {
                    0
                } else if properties.device_type == vk::PhysicalDeviceType::DISCRETE_GPU {
                    1
                } else if properties.device_type == vk::PhysicalDeviceType::INTEGRATED_GPU {
                    2
                } else {
                    0
                };
                (format!("Vulkan{index}"), description, kind)
            })
            .collect())
    })();
    instance.destroy_instance(None);
    result
}

// Keep native backend enumeration inside the disposable inference process.
#[cfg(target_os = "linux")]
unsafe fn select_vulkan_device(lib: &Library) -> Result<()> {
    let requested = std::env::var("PARAKEET_DEVICE").ok();
    if requested.as_deref() == Some("cpu") {
        return Ok(());
    }
    let count: unsafe extern "C" fn() -> usize = sym(lib, b"ggml_backend_dev_count\0")?;
    let get: unsafe extern "C" fn(usize) -> Ctx = sym(lib, b"ggml_backend_dev_get\0")?;
    let name: unsafe extern "C" fn(Ctx) -> *const c_char = sym(lib, b"ggml_backend_dev_name\0")?;
    let description: unsafe extern "C" fn(Ctx) -> *const c_char =
        sym(lib, b"ggml_backend_dev_description\0")?;
    let device_type: unsafe extern "C" fn(Ctx) -> c_int = sym(lib, b"ggml_backend_dev_type\0")?;
    let mut candidates = Vec::new();
    for index in 0..count() {
        let device = get(index);
        let device_name = CStr::from_ptr(name(device)).to_string_lossy().into_owned();
        let device_description = CStr::from_ptr(description(device))
            .to_string_lossy()
            .into_owned();
        candidates.push((device_name, device_description, device_type(device)));
    }
    if let Some(index) = preferred_vulkan_device(&candidates) {
        let (name, description, _) = &candidates[index];
        std::env::set_var("PARAKEET_DEVICE", name);
        tracing::info!(device = %name, gpu = %description, "selected physical Vulkan GPU");
    } else {
        anyhow::ensure!(
            requested.is_none(),
            "GPU mode requires a physical Vulkan GPU; no GPU was found"
        );
        // Automatic mode may fall back to CPU, explicit GPU mode may not.
        std::env::set_var("PARAKEET_DEVICE", "cpu");
    }
    Ok(())
}

#[cfg(any(windows, target_os = "linux"))]
fn preferred_vulkan_device(devices: &[(String, String, c_int)]) -> Option<usize> {
    // ggml device types: 0 = CPU, 1 = discrete GPU, 2 = integrated GPU.
    // Software Vulkan implementations must never satisfy explicit GPU mode.
    devices
        .iter()
        .enumerate()
        .filter(|(_, (name, description, kind))| {
            let description = description.to_ascii_lowercase();
            name.starts_with("Vulkan")
                && matches!(kind, 1 | 2)
                && !description.contains("llvmpipe")
                && !description.contains("lavapipe")
                && !description.contains("software")
        })
        .min_by_key(|(_, (_, _, kind))| if *kind == 1 { 0 } else { 1 })
        .map(|(index, _)| index)
}

#[cfg(all(test, any(windows, target_os = "linux")))]
mod gpu_tests {
    use super::*;

    #[test]
    fn discrete_gpu_wins_over_integrated_and_software_devices() {
        let devices = vec![
            ("Vulkan0".into(), "AMD Radeon integrated".into(), 2),
            ("Vulkan1".into(), "NVIDIA RTX 3050 Ti".into(), 1),
            ("Vulkan2".into(), "llvmpipe".into(), 1),
            ("CPU".into(), "AMD Ryzen".into(), 0),
        ];
        assert_eq!(preferred_vulkan_device(&devices), Some(1));
        assert_eq!(preferred_vulkan_device(&devices[..1]), Some(0));
        assert_eq!(preferred_vulkan_device(&devices[2..]), None);
        assert_eq!(preferred_vulkan_device(&[]), None);
    }
}

unsafe fn sym<T: Copy>(lib: &Library, name: &[u8]) -> Result<T> {
    Ok(*lib.get::<T>(name)?)
}

fn create_context(api: &Api, model_path: &Path) -> Result<Ctx> {
    anyhow::ensure!(
        model_path.exists(),
        "Parakeet GGUF model is missing: {}",
        model_path.display()
    );
    let model = CString::new(model_path.to_string_lossy().as_bytes())
        .context("model path contains an interior NUL byte")?;
    let ctx = unsafe { (api.load)(model.as_ptr()) };
    anyhow::ensure!(!ctx.is_null(), "parakeet_capi_load returned null");
    Ok(ctx)
}

fn parakeet_library_path(runtime_dir: &Path) -> Result<PathBuf> {
    let mut candidates = Vec::new();
    #[cfg(windows)]
    {
        candidates.push(runtime_dir.join("bin").join("parakeet.dll"));
        candidates.push(runtime_dir.join("parakeet.dll"));
    }
    #[cfg(target_os = "linux")]
    {
        candidates.push(runtime_dir.join("bin").join("libparakeet.so"));
        candidates.push(runtime_dir.join("lib").join("libparakeet.so"));
        candidates.push(runtime_dir.join("libparakeet.so"));
        candidates.push(runtime_dir.join("bin").join("parakeet.so"));
        candidates.push(runtime_dir.join("lib").join("parakeet.so"));
        candidates.push(runtime_dir.join("parakeet.so"));
    }
    #[cfg(target_os = "macos")]
    {
        candidates.push(runtime_dir.join("bin").join("libparakeet.dylib"));
        candidates.push(runtime_dir.join("lib").join("libparakeet.dylib"));
        candidates.push(runtime_dir.join("libparakeet.dylib"));
    }
    for path in &candidates {
        if path.exists() {
            return Ok(path.clone());
        }
    }
    let searched = candidates
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    anyhow::bail!("Parakeet native library is missing; searched: {searched}")
}

#[cfg(windows)]
fn configure_native_search(runtime_dir: &Path) -> Result<()> {
    let bin = runtime_dir.join("bin");
    anyhow::ensure!(
        bin.exists(),
        "Parakeet bin directory is missing: {}",
        bin.display()
    );
    unsafe {
        anyhow::ensure!(
            SetDefaultDllDirectories(
                LOAD_LIBRARY_SEARCH_DEFAULT_DIRS | LOAD_LIBRARY_SEARCH_USER_DIRS
            ) != 0,
            "SetDefaultDllDirectories failed"
        );
        let wide: Vec<u16> = bin
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        anyhow::ensure!(
            !AddDllDirectory(wide.as_ptr()).is_null(),
            "AddDllDirectory failed for {}",
            bin.display()
        );
    }
    Ok(())
}

#[cfg(not(windows))]
fn configure_native_search(_: &Path) -> Result<()> {
    Ok(())
}
