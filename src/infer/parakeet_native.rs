#[cfg(windows)]
mod platform {
    use anyhow::{anyhow, Context, Result};
    use libloading::Library;
    use std::ffi::{c_char, c_float, c_int, c_void, CStr, CString};
    use std::path::Path;
    use std::ptr::null_mut;
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
        transcribe_pcm:
            unsafe extern "C" fn(Ctx, *const c_float, c_int, c_int, c_int) -> *mut c_char,
        free_string: unsafe extern "C" fn(*mut c_char),
        last_error: unsafe extern "C" fn(Ctx) -> *const c_char,
    }
    impl Api {
        fn load(runtime_dir: &Path) -> Result<Self> {
            let bin = runtime_dir.join("bin");
            anyhow::ensure!(
                bin.exists(),
                "Parakeet bin directory is missing: {}",
                bin.display()
            );
            configure_dll_search(&bin)?;
            let dll = bin.join("parakeet.dll");
            anyhow::ensure!(dll.exists(), "missing {}", dll.display());
            let lib = unsafe { Library::new(&dll) }
                .with_context(|| format!("loading {}", dll.display()))?;
            unsafe {
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
    fn configure_dll_search(bin: &Path) -> Result<()> {
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
}

#[cfg(not(windows))]
mod platform {
    use anyhow::{bail, Result};
    use std::path::Path;
    pub struct ParakeetNative;
    impl ParakeetNative {
        pub fn load(_: &Path, _: &Path) -> Result<Self> {
            bail!("Parakeet inference worker is Windows-only")
        }
        pub fn transcribe_wav(&self, _: &Path) -> Result<String> {
            bail!("Parakeet inference worker is Windows-only")
        }
        pub fn transcribe_pcm16_16k(&self, _: &[i16]) -> Result<String> {
            bail!("Parakeet inference worker is Windows-only")
        }
    }
}

pub use platform::ParakeetNative;
