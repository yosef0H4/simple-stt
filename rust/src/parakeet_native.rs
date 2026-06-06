use anyhow::{anyhow, Context, Result};
use libloading::Library;
use std::ffi::{c_char, c_float, c_int, c_void, CStr, CString};
use std::path::{Path, PathBuf};
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
    pub fn load_default() -> Result<Self> {
        Self::load(&default_model_path()?)
    }

    pub fn load(model_path: &Path) -> Result<Self> {
        anyhow::ensure!(
            model_path.exists(),
            "Parakeet GGUF model is missing: {}",
            model_path.display()
        );
        let api = Api::load()?;
        let model = CString::new(model_path.to_string_lossy().as_bytes())
            .context("model path contains an interior NUL byte")?;
        let ctx = unsafe { (api.load)(model.as_ptr()) };
        if ctx.is_null() {
            return Err(anyhow!("parakeet_capi_load returned null"));
        }
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
        let text = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
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
    fn load() -> Result<Self> {
        let bin = runtime_bin_dir()?;
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

pub fn runtime_root() -> Result<PathBuf> {
    let root = crate::config::repo_root()
        .join("external")
        .join("parakeet-runtime")
        .join("parakeet-windows-cuda");
    anyhow::ensure!(
        root.exists(),
        "Parakeet runtime is missing: {}",
        root.display()
    );
    Ok(root)
}

fn runtime_bin_dir() -> Result<PathBuf> {
    let bin = runtime_root()?.join("bin");
    anyhow::ensure!(bin.exists(), "Parakeet bin directory is missing: {}", bin.display());
    Ok(bin)
}

fn default_model_path() -> Result<PathBuf> {
    Ok(runtime_root()?.join("models").join("tdt_ctc-110m-f16.gguf"))
}

fn configure_dll_search(bin: &Path) -> Result<()> {
    unsafe {
        let ok = SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS | LOAD_LIBRARY_SEARCH_USER_DIRS);
        anyhow::ensure!(ok != 0, "SetDefaultDllDirectories failed");
        let wide = wide_null(&bin.to_string_lossy());
        let cookie = AddDllDirectory(wide.as_ptr());
        anyhow::ensure!(!cookie.is_null(), "AddDllDirectory failed for {}", bin.display());
    }
    Ok(())
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
