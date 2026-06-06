use anyhow::{anyhow, Context, Result};
use libloading::Library;
use std::ffi::{c_char, c_void, CString};
use std::fs;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use tracing::{debug, info};
use windows_sys::Win32::System::LibraryLoader::{
    AddDllDirectory, SetDefaultDllDirectories, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
    LOAD_LIBRARY_SEARCH_USER_DIRS,
};

type RawHandle = *mut c_void;
type HResult = isize;

const PROCESSOR_ARCHITECTURE_X64: u32 = 4;
const SPEECH_PACKAGE_PREFIX: &str = "MicrosoftWindows.Speech.en-US.1_";
const SPEECH_FAMILY_SUFFIX: &str = "_cw5n1h2txyewy";

pub struct LiveCaptionsNative {
    sdk: SpeechSdk,
    _graph: PackageGraph,
    model_root: PathBuf,
    model_name: String,
    license: String,
}

impl LiveCaptionsNative {
    pub fn new() -> Result<Self> {
        let sdk_dir = speech_sdk_dir()?;
        configure_dll_search(&sdk_dir)?;
        let model_root = find_model_root()?;
        let graph = PackageGraph::for_model_root(&model_root)?;
        let sdk = SpeechSdk::load(&sdk_dir)?;
        let license = read_license(&model_root)?;
        let model_name = sdk.find_model_name(&model_root, &license)?;
        info!(
            sdk_dir = %sdk_dir.display(),
            model_root = %model_root.display(),
            model_name,
            license_len = license.len(),
            "loaded native Live Captions Speech SDK"
        );
        Ok(Self {
            sdk,
            _graph: graph,
            model_root,
            model_name,
            license,
        })
    }

    pub fn recognize_wav_once(&mut self, audio: &Path) -> Result<String> {
        anyhow::ensure!(audio.exists(), "audio file does not exist: {}", audio.display());
        let pcm = read_pcm16_wav(audio)?;
        let mut config = self.create_config(false)?;
        let mut format_handle = null_mut();
        self.sdk.ok(
            unsafe {
                (self.sdk.audio_stream_format_create_from_waveformat)(
                    &mut format_handle,
                    16_000,
                    16,
                    1,
                    1,
                )
            },
            "audio_stream_format_create_from_waveformat",
        )?;
        let mut format = HandleGuard::new(format_handle, self.sdk.audio_stream_format_release);
        let mut stream_handle = null_mut();
        self.sdk.ok(
            unsafe { (self.sdk.audio_stream_create_push_audio_input_stream)(&mut stream_handle, format.as_raw()) },
            "audio_stream_create_push_audio_input_stream",
        )?;
        let mut stream = HandleGuard::new(stream_handle, self.sdk.audio_stream_release);
        let mut audio_handle = null_mut();
        self.sdk.ok(
            unsafe { (self.sdk.audio_config_create_audio_input_from_stream)(&mut audio_handle, stream.as_raw()) },
            "audio_config_create_audio_input_from_stream",
        )?;
        anyhow::ensure!(
            unsafe { (self.sdk.audio_config_is_handle_valid)(audio_handle) },
            "SDK reported push-stream audio config handle is invalid"
        );
        let mut audio = HandleGuard::new(audio_handle, self.sdk.audio_config_release);
        let mut reco_handle = null_mut();
        self.sdk.ok(
            unsafe { (self.sdk.recognizer_create_speech_recognizer_from_config)(&mut reco_handle, config.as_raw(), audio.as_raw()) },
            "recognizer_create_speech_recognizer_from_config",
        )?;
        let mut reco = HandleGuard::new(reco_handle, self.sdk.recognizer_handle_release);
        self.sdk.ok(
            unsafe { (self.sdk.push_audio_input_stream_write)(stream.as_raw(), pcm.as_ptr(), pcm.len() as u32) },
            "push_audio_input_stream_write",
        )?;
        self.sdk.ok(
            unsafe { (self.sdk.push_audio_input_stream_close)(stream.as_raw()) },
            "push_audio_input_stream_close",
        )?;
        let text = self.recognize_once_with_reco(reco.as_raw())?;
        reco.release();
        audio.release();
        stream.release();
        format.release();
        config.release();
        Ok(text)
    }

    pub fn recognize_default_microphone_once(&mut self) -> Result<String> {
        let mut config = self.create_config(true)?;
        let mut audio_handle = null_mut();
        self.sdk
            .ok(unsafe { (self.sdk.audio_config_create_audio_input_from_default_microphone)(&mut audio_handle) }, "audio_config_create_audio_input_from_default_microphone")?;
        anyhow::ensure!(
            unsafe { (self.sdk.audio_config_is_handle_valid)(audio_handle) },
            "SDK reported microphone audio config handle is invalid"
        );
        let mut audio = HandleGuard::new(audio_handle, self.sdk.audio_config_release);
        let text = self.recognize_once(config.as_raw(), audio.as_raw())?;
        audio.release();
        config.release();
        Ok(text)
    }

    fn create_config(&self, microphone: bool) -> Result<HandleGuard> {
        let mut config_handle = null_mut();
        self.sdk.ok(
            unsafe { (self.sdk.embedded_speech_config_create)(&mut config_handle) },
            "embedded_speech_config_create",
        )?;
        anyhow::ensure!(
            unsafe { (self.sdk.speech_config_is_handle_valid)(config_handle) },
            "SDK reported embedded speech config handle is invalid after create"
        );
        let config = HandleGuard::new(config_handle, self.sdk.config_release);
        let path = cstring_path(&self.model_root)?;
        self.sdk.ok(
            unsafe { (self.sdk.embedded_speech_config_add_path)(config.as_raw(), path.as_ptr()) },
            "embedded_speech_config_add_path",
        )?;
        let name = CString::new(self.model_name.as_str())?;
        let license = CString::new(self.license.as_str())?;
        self.sdk.ok(
            unsafe {
                (self.sdk.embedded_speech_config_set_speech_recognition_model)(
                    config.as_raw(),
                    name.as_ptr(),
                    license.as_ptr(),
                )
            },
            "embedded_speech_config_set_speech_recognition_model",
        )?;
        anyhow::ensure!(
            unsafe { (self.sdk.speech_config_is_handle_valid)(config.as_raw()) },
            "SDK reported embedded speech config handle is invalid after model selection"
        );
        self.set_property(config.as_raw(), "SpeechRecognition_SegmentationFlavor", "aggressive")?;
        self.set_property(config.as_raw(), "SpeechRecognition_PunctuationMode", "explicit")?;
        self.set_property(config.as_raw(), "SpeechRecognition_RequestPerformanceMetrics", "true")?;
        self.set_property(
            config.as_raw(),
            "SpeechRecognition_RequestWordLevelCorrections",
            if microphone { "false" } else { "true" },
        )?;
        self.set_property(config.as_raw(), "SpeechServiceResponse_RequestProfanityFilterTrueFalse", "false")?;
        Ok(config)
    }

    fn set_property(&self, config: RawHandle, name: &str, value: &str) -> Result<()> {
        let mut propbag = null_mut();
        self.sdk.ok(
            unsafe { (self.sdk.speech_config_get_property_bag)(config, &mut propbag) },
            "speech_config_get_property_bag",
        )?;
        let name = CString::new(name)?;
        let value = CString::new(value)?;
        self.sdk.ok(
            unsafe { (self.sdk.property_bag_set_string)(propbag, -1, name.as_ptr() as _, value.as_ptr() as _) },
            "property_bag_set_string",
        )
    }

    fn recognize_once(&self, config: RawHandle, audio: RawHandle) -> Result<String> {
        let mut reco_handle = null_mut();
        self.sdk.ok(
            unsafe { (self.sdk.recognizer_create_speech_recognizer_from_config)(&mut reco_handle, config, audio) },
            "recognizer_create_speech_recognizer_from_config",
        )?;
        let mut reco = HandleGuard::new(reco_handle, self.sdk.recognizer_handle_release);
        let text = self.recognize_once_with_reco(reco.as_raw())?;
        reco.release();
        Ok(text)
    }

    fn recognize_once_with_reco(&self, reco: RawHandle) -> Result<String> {
        let mut result_handle = null_mut();
        self.sdk.ok(
            unsafe { (self.sdk.recognizer_recognize_once)(reco, &mut result_handle) },
            "recognizer_recognize_once",
        )?;
        let mut result = HandleGuard::new(result_handle, self.sdk.recognizer_result_handle_release);
        let mut reason = 0_i32;
        self.sdk.ok(
            unsafe { (self.sdk.result_get_reason)(result.as_raw(), &mut reason) },
            "result_get_reason",
        )?;
        let text = self.sdk.result_text(result.as_raw())?;
        debug!(reason, text, "native Live Captions recognition result");
        result.release();
        Ok(text)
    }
}

struct SpeechSdk {
    _lib: Library,
    embedded_speech_config_create: unsafe extern "system" fn(*mut RawHandle) -> HResult,
    embedded_speech_config_add_path: unsafe extern "system" fn(RawHandle, *const c_char) -> HResult,
    embedded_speech_config_get_num_speech_reco_models:
        unsafe extern "system" fn(RawHandle, *mut u32) -> HResult,
    embedded_speech_config_get_speech_reco_model:
        unsafe extern "system" fn(RawHandle, u32, *mut RawHandle) -> HResult,
    embedded_speech_config_set_speech_recognition_model:
        unsafe extern "system" fn(RawHandle, *const c_char, *const c_char) -> HResult,
    speech_recognition_model_get_name: unsafe extern "system" fn(RawHandle) -> *mut c_char,
    speech_recognition_model_handle_release: unsafe extern "system" fn(RawHandle) -> HResult,
    speech_config_is_handle_valid: unsafe extern "system" fn(RawHandle) -> bool,
    config_release: unsafe extern "system" fn(RawHandle) -> HResult,
    speech_config_get_property_bag: unsafe extern "system" fn(RawHandle, *mut RawHandle) -> HResult,
    audio_config_create_audio_input_from_stream:
        unsafe extern "system" fn(*mut RawHandle, RawHandle) -> HResult,
    audio_config_create_audio_input_from_default_microphone:
        unsafe extern "system" fn(*mut RawHandle) -> HResult,
    audio_config_release: unsafe extern "system" fn(RawHandle) -> HResult,
    audio_config_is_handle_valid: unsafe extern "system" fn(RawHandle) -> bool,
    audio_stream_format_create_from_waveformat:
        unsafe extern "system" fn(*mut RawHandle, u32, u8, u8, i32) -> HResult,
    audio_stream_format_release: unsafe extern "system" fn(RawHandle) -> HResult,
    audio_stream_create_push_audio_input_stream:
        unsafe extern "system" fn(*mut RawHandle, RawHandle) -> HResult,
    push_audio_input_stream_write:
        unsafe extern "system" fn(RawHandle, *const u8, u32) -> HResult,
    push_audio_input_stream_close: unsafe extern "system" fn(RawHandle) -> HResult,
    audio_stream_release: unsafe extern "system" fn(RawHandle) -> HResult,
    recognizer_create_speech_recognizer_from_config:
        unsafe extern "system" fn(*mut RawHandle, RawHandle, RawHandle) -> HResult,
    recognizer_recognize_once: unsafe extern "system" fn(RawHandle, *mut RawHandle) -> HResult,
    recognizer_handle_release: unsafe extern "system" fn(RawHandle) -> HResult,
    recognizer_result_handle_release: unsafe extern "system" fn(RawHandle) -> HResult,
    result_get_text: unsafe extern "system" fn(RawHandle, *mut c_char, u32) -> HResult,
    result_get_reason: unsafe extern "system" fn(RawHandle, *mut i32) -> HResult,
    property_bag_set_string:
        unsafe extern "system" fn(RawHandle, i32, *const c_void, *const c_void) -> HResult,
    property_bag_free_string: unsafe extern "system" fn(*mut c_char) -> HResult,
    error_get_message: unsafe extern "system" fn(RawHandle) -> *mut c_char,
    error_get_error_code: unsafe extern "system" fn(RawHandle) -> isize,
    error_release: unsafe extern "system" fn(RawHandle) -> HResult,
}

impl SpeechSdk {
    fn load(sdk_dir: &Path) -> Result<Self> {
        let core = sdk_dir.join("Microsoft.CognitiveServices.Speech.core.dll");
        anyhow::ensure!(core.exists(), "missing Speech SDK core DLL: {}", core.display());
        let lib = unsafe { Library::new(&core) }
            .with_context(|| format!("loading {}", core.display()))?;
        unsafe {
            Ok(Self {
                embedded_speech_config_create: sym(&lib, b"embedded_speech_config_create\0")?,
                embedded_speech_config_add_path: sym(&lib, b"embedded_speech_config_add_path\0")?,
                embedded_speech_config_get_num_speech_reco_models: sym(
                    &lib,
                    b"embedded_speech_config_get_num_speech_reco_models\0",
                )?,
                embedded_speech_config_get_speech_reco_model: sym(
                    &lib,
                    b"embedded_speech_config_get_speech_reco_model\0",
                )?,
                embedded_speech_config_set_speech_recognition_model: sym(
                    &lib,
                    b"embedded_speech_config_set_speech_recognition_model\0",
                )?,
                speech_recognition_model_get_name: sym(&lib, b"speech_recognition_model_get_name\0")?,
                speech_recognition_model_handle_release: sym(
                    &lib,
                    b"speech_recognition_model_handle_release\0",
                )?,
                speech_config_is_handle_valid: sym(&lib, b"speech_config_is_handle_valid\0")?,
                config_release: sym(&lib, b"speech_config_release\0")?,
                speech_config_get_property_bag: sym(&lib, b"speech_config_get_property_bag\0")?,
                audio_config_create_audio_input_from_stream: sym(
                    &lib,
                    b"audio_config_create_audio_input_from_stream\0",
                )?,
                audio_config_create_audio_input_from_default_microphone: sym(
                    &lib,
                    b"audio_config_create_audio_input_from_default_microphone\0",
                )?,
                audio_config_release: sym(&lib, b"audio_config_release\0")?,
                audio_config_is_handle_valid: sym(&lib, b"audio_config_is_handle_valid\0")?,
                audio_stream_format_create_from_waveformat: sym(
                    &lib,
                    b"audio_stream_format_create_from_waveformat\0",
                )?,
                audio_stream_format_release: sym(&lib, b"audio_stream_format_release\0")?,
                audio_stream_create_push_audio_input_stream: sym(
                    &lib,
                    b"audio_stream_create_push_audio_input_stream\0",
                )?,
                push_audio_input_stream_write: sym(&lib, b"push_audio_input_stream_write\0")?,
                push_audio_input_stream_close: sym(&lib, b"push_audio_input_stream_close\0")?,
                audio_stream_release: sym(&lib, b"audio_stream_release\0")?,
                recognizer_create_speech_recognizer_from_config: sym(
                    &lib,
                    b"recognizer_create_speech_recognizer_from_config\0",
                )?,
                recognizer_recognize_once: sym(&lib, b"recognizer_recognize_once\0")?,
                recognizer_handle_release: sym(&lib, b"recognizer_handle_release\0")?,
                recognizer_result_handle_release: sym(&lib, b"recognizer_result_handle_release\0")?,
                result_get_text: sym(&lib, b"result_get_text\0")?,
                result_get_reason: sym(&lib, b"result_get_reason\0")?,
                property_bag_set_string: sym(&lib, b"property_bag_set_string\0")?,
                property_bag_free_string: sym(&lib, b"property_bag_free_string\0")?,
                error_get_message: sym(&lib, b"error_get_message\0")?,
                error_get_error_code: sym(&lib, b"error_get_error_code\0")?,
                error_release: sym(&lib, b"error_release\0")?,
                _lib: lib,
            })
        }
    }

    fn find_model_name(&self, model_root: &Path, license: &str) -> Result<String> {
        let mut config_handle = null_mut();
        self.ok(
            unsafe { (self.embedded_speech_config_create)(&mut config_handle) },
            "embedded_speech_config_create",
        )?;
        let mut config = HandleGuard::new(config_handle, self.config_release);
        let path = cstring_path(model_root)?;
        self.ok(
            unsafe { (self.embedded_speech_config_add_path)(config.as_raw(), path.as_ptr()) },
            "embedded_speech_config_add_path",
        )?;
        let mut count = 0_u32;
        self.ok(
            unsafe { (self.embedded_speech_config_get_num_speech_reco_models)(config.as_raw(), &mut count) },
            "embedded_speech_config_get_num_speech_reco_models",
        )?;
        anyhow::ensure!(count > 0, "no speech recognition models found under {}", model_root.display());
        let mut model = null_mut();
        self.ok(
            unsafe { (self.embedded_speech_config_get_speech_reco_model)(config.as_raw(), 0, &mut model) },
            "embedded_speech_config_get_speech_reco_model",
        )?;
        let mut model = HandleGuard::new(model, self.speech_recognition_model_handle_release);
        let name = self.model_name(model.as_raw())?;
        let name_c = CString::new(name.as_str())?;
        let license_c = CString::new(license)?;
        self.ok(
            unsafe {
                (self.embedded_speech_config_set_speech_recognition_model)(
                    config.as_raw(),
                    name_c.as_ptr(),
                    license_c.as_ptr(),
                )
            },
            "embedded_speech_config_set_speech_recognition_model",
        )?;
        model.release();
        config.release();
        Ok(name)
    }

    fn model_name(&self, model: RawHandle) -> Result<String> {
        let ptr = unsafe { (self.speech_recognition_model_get_name)(model) };
        anyhow::ensure!(!ptr.is_null(), "speech_recognition_model_get_name returned null");
        let value = c_ptr_to_string(ptr);
        self.ok(
            unsafe { (self.property_bag_free_string)(ptr) },
            "property_bag_free_string",
        )?;
        Ok(value)
    }

    fn result_text(&self, result: RawHandle) -> Result<String> {
        let mut buffer = vec![0_i8; 2048];
        self.ok(
            unsafe { (self.result_get_text)(result, buffer.as_mut_ptr(), buffer.len() as u32) },
            "result_get_text",
        )?;
        Ok(c_buffer_to_string(&buffer))
    }

    fn ok(&self, hr: HResult, operation: &str) -> Result<()> {
        if hr == 0 {
            Ok(())
        } else {
            let error = hr as RawHandle;
            let raw_code = unsafe { (self.error_get_error_code)(error) };
            let code = if raw_code == 0 { hr } else { raw_code };
            let message_ptr = unsafe { (self.error_get_message)(error) };
            let message = if message_ptr.is_null() {
                String::new()
            } else {
                c_ptr_to_string(message_ptr)
            };
            let _ = unsafe { (self.error_release)(error) };
            if message.is_empty() {
                Err(anyhow!(
                    "{operation} failed with Speech SDK error code 0x{:08X}",
                    code as u32
                ))
            } else {
                Err(anyhow!(
                    "{operation} failed with Speech SDK error code 0x{:08X}: {message}",
                    code as u32
                ))
            }
        }
    }
}

unsafe fn sym<T: Copy>(lib: &Library, name: &[u8]) -> Result<T> {
    Ok(*lib.get::<T>(name)?)
}

struct HandleGuard {
    handle: RawHandle,
    release: unsafe extern "system" fn(RawHandle) -> HResult,
}

impl HandleGuard {
    fn new(handle: RawHandle, release: unsafe extern "system" fn(RawHandle) -> HResult) -> Self {
        Self { handle, release }
    }

    fn as_raw(&self) -> RawHandle {
        self.handle
    }

    fn release(&mut self) {
        if !self.handle.is_null() {
            let handle = std::mem::replace(&mut self.handle, null_mut());
            unsafe {
                let _ = (self.release)(handle);
            }
        }
    }
}

impl Drop for HandleGuard {
    fn drop(&mut self) {
        self.release();
    }
}

struct PackageGraph {
    api: Option<PackageApi>,
    context: isize,
    dependency_id: Option<Vec<u16>>,
}

impl PackageGraph {
    fn for_model_root(model_root: &Path) -> Result<Self> {
        let root = model_root.to_string_lossy();
        if !root.contains("\\WindowsApps\\MicrosoftWindows.Speech.") {
            return Ok(Self {
                api: None,
                context: 0,
                dependency_id: None,
            });
        }
        let api = PackageApi::load()?;
        let dir = model_root
            .file_name()
            .and_then(|name| name.to_str())
            .context("speech model root has no directory name")?;
        let family = match dir.find("_1.") {
            Some(index) => format!("{}{}", &dir[..index], SPEECH_FAMILY_SUFFIX),
            None => dir.to_string(),
        };
        let family_w = wide_null(&family);
        let mut dependency_id_ptr: *mut u16 = null_mut();
        let create_hr = unsafe {
            (api.try_create_package_dependency)(
                null(),
                family_w.as_ptr(),
                0,
                PROCESSOR_ARCHITECTURE_X64,
                0,
                null(),
                0,
                &mut dependency_id_ptr,
            )
        };
        anyhow::ensure!(
            create_hr == 0,
            "TryCreatePackageDependency({family}) failed with HRESULT 0x{:08X}",
            create_hr as u32
        );
        let dependency_id = unsafe { wide_from_ptr(dependency_id_ptr) };
        let mut context = 0_isize;
        let mut full_name: *mut u16 = null_mut();
        let add_hr = unsafe {
            (api.add_package_dependency)(dependency_id.as_ptr(), 0, 0, &mut context, &mut full_name)
        };
        anyhow::ensure!(
            add_hr == 0,
            "AddPackageDependency({family}) failed with HRESULT 0x{:08X}",
            add_hr as u32
        );
        debug!(family, "attached Windows speech package graph");
        Ok(Self {
            api: Some(api),
            context,
            dependency_id: Some(dependency_id),
        })
    }
}

impl Drop for PackageGraph {
    fn drop(&mut self) {
        unsafe {
            if let Some(api) = &self.api {
                if self.context != 0 {
                    (api.remove_package_dependency)(self.context);
                    self.context = 0;
                }
                if let Some(id) = &self.dependency_id {
                    let _ = (api.delete_package_dependency)(id.as_ptr());
                }
            }
        }
    }
}

struct PackageApi {
    _lib: Library,
    try_create_package_dependency: unsafe extern "system" fn(
        *const c_void,
        *const u16,
        u64,
        u32,
        u32,
        *const u16,
        u32,
        *mut *mut u16,
    ) -> i32,
    add_package_dependency:
        unsafe extern "system" fn(*const u16, i32, u32, *mut isize, *mut *mut u16) -> i32,
    remove_package_dependency: unsafe extern "system" fn(isize),
    delete_package_dependency: unsafe extern "system" fn(*const u16) -> i32,
}

impl PackageApi {
    fn load() -> Result<Self> {
        let lib = unsafe { Library::new("kernelbase.dll") }.context("loading kernelbase.dll")?;
        unsafe {
            Ok(Self {
                try_create_package_dependency: sym(&lib, b"TryCreatePackageDependency\0")?,
                add_package_dependency: sym(&lib, b"AddPackageDependency\0")?,
                remove_package_dependency: sym(&lib, b"RemovePackageDependency\0")?,
                delete_package_dependency: sym(&lib, b"DeletePackageDependency\0")?,
                _lib: lib,
            })
        }
    }
}

fn speech_sdk_dir() -> Result<PathBuf> {
    let dir = crate::config::repo_root()
        .join("external")
        .join("windows-live-captions-stt")
        .join(".build")
        .join("windows-live-captions-stt-helper");
    anyhow::ensure!(
        dir.join("Microsoft.CognitiveServices.Speech.core.dll").exists(),
        "native Speech SDK DLLs are missing from {}",
        dir.display()
    );
    Ok(dir)
}

fn configure_dll_search(sdk_dir: &Path) -> Result<()> {
    unsafe {
        let ok = SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS | LOAD_LIBRARY_SEARCH_USER_DIRS);
        anyhow::ensure!(ok != 0, "SetDefaultDllDirectories failed");
        add_dll_dir(sdk_dir)?;
        add_dll_dir(Path::new(
            r"C:\Windows\SystemApps\MicrosoftWindows.Client.Core_cw5n1h2txyewy\LiveCaptions",
        ))?;
        add_dll_dir(Path::new(
            r"C:\Windows\SystemApps\MicrosoftWindows.Client.Core_cw5n1h2txyewy",
        ))?;
    }
    Ok(())
}

unsafe fn add_dll_dir(path: &Path) -> Result<()> {
    if path.exists() {
        let wide = wide_null_path(path);
        let cookie = AddDllDirectory(wide.as_ptr());
        anyhow::ensure!(
            !cookie.is_null(),
            "AddDllDirectory failed for {}",
            path.display()
        );
    }
    Ok(())
}

fn find_model_root() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("UVOX_LIVE_CAPTIONS_MODEL_ROOT") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Ok(path);
        }
    }
    let known = PathBuf::from(
        r"C:\Program Files\WindowsApps\MicrosoftWindows.Speech.en-US.1_1.0.28.0_x64__cw5n1h2txyewy",
    );
    if known.exists() {
        return Ok(known);
    }
    let windows_apps = Path::new(r"C:\Program Files\WindowsApps");
    let mut candidates = Vec::new();
    for entry in fs::read_dir(windows_apps).context("reading C:\\Program Files\\WindowsApps")? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(SPEECH_PACKAGE_PREFIX) && name.ends_with(SPEECH_FAMILY_SUFFIX) {
            candidates.push(entry.path());
        }
    }
    candidates.sort();
    candidates
        .pop()
        .ok_or_else(|| anyhow!("no MicrosoftWindows.Speech.en-US package found under WindowsApps"))
}

fn read_license(model_root: &Path) -> Result<String> {
    for candidate in [
        r"C:\Windows\SystemApps\MicrosoftWindows.Client.Core_cw5n1h2txyewy\SpeechRecognizer.dll",
        r"C:\Windows\SystemApps\MicrosoftWindows.Client.Core_cw5n1h2txyewy\LiveCaptionsBackendDll.dll",
    ] {
        let path = Path::new(candidate);
        if path.exists() {
            let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
            if let Some(value) = extract_license(&bytes, b"This model and the software may not be used", 1) {
                return Ok(value);
            }
            let marker: Vec<u8> = "This model and the software may not be used"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect();
            if let Some(value) = extract_license_utf16(&bytes, &marker) {
                return Ok(value);
            }
        }
    }
    let version = model_root.join("version.txt");
    if version.exists() {
        let content = fs::read_to_string(&version)?;
        if let Some(line) = content.lines().nth(1) {
            return Ok(line.trim().to_string());
        }
    }
    Ok(String::new())
}

fn read_pcm16_wav(path: &Path) -> Result<Vec<u8>> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("opening WAV file {}", path.display()))?;
    let spec = reader.spec();
    anyhow::ensure!(
        spec.channels == 1
            && spec.sample_rate == 16_000
            && spec.bits_per_sample == 16
            && spec.sample_format == hound::SampleFormat::Int,
        "native file test expects 16 kHz mono PCM16 WAV; got {} Hz, {} channel(s), {} bits, {:?}",
        spec.sample_rate,
        spec.channels,
        spec.bits_per_sample,
        spec.sample_format
    );
    let mut pcm = Vec::new();
    for sample in reader.samples::<i16>() {
        pcm.extend_from_slice(&sample?.to_le_bytes());
    }
    Ok(pcm)
}

fn extract_license(bytes: &[u8], marker: &[u8], terminator_width: usize) -> Option<String> {
    let index = find_bytes(bytes, marker)?;
    let mut start = index;
    while start >= terminator_width && !is_zero_at(bytes, start - terminator_width, terminator_width) {
        start -= terminator_width;
    }
    let mut end = index + marker.len();
    while end + terminator_width <= bytes.len() && !is_zero_at(bytes, end, terminator_width) {
        end += terminator_width;
    }
    String::from_utf8(bytes[start..end].to_vec()).ok()
}

fn extract_license_utf16(bytes: &[u8], marker: &[u8]) -> Option<String> {
    let index = find_bytes(bytes, marker)?;
    let mut start = index;
    while start >= 2 && !is_zero_at(bytes, start - 2, 2) {
        start -= 2;
    }
    let mut end = index + marker.len();
    while end + 2 <= bytes.len() && !is_zero_at(bytes, end, 2) {
        end += 2;
    }
    let words: Vec<u16> = bytes[start..end]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16(&words).ok()
}

fn find_bytes(bytes: &[u8], pattern: &[u8]) -> Option<usize> {
    bytes.windows(pattern.len()).position(|window| window == pattern)
}

fn is_zero_at(bytes: &[u8], offset: usize, width: usize) -> bool {
    bytes
        .get(offset..offset + width)
        .is_some_and(|slice| slice.iter().all(|byte| *byte == 0))
}

fn c_buffer_to_string(buffer: &[i8]) -> String {
    let bytes: Vec<u8> = buffer
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| *byte as u8)
        .collect();
    String::from_utf8_lossy(&bytes).trim().to_string()
}

fn c_ptr_to_string(ptr: *const c_char) -> String {
    let mut len = 0;
    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let bytes = std::slice::from_raw_parts(ptr as *const u8, len);
        String::from_utf8_lossy(bytes).trim().to_string()
    }
}

fn cstring_path(path: &Path) -> Result<CString> {
    CString::new(path.to_string_lossy().as_bytes()).context("path contains an interior NUL byte")
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_null_path(path: &Path) -> Vec<u16> {
    wide_null(&path.to_string_lossy())
}

unsafe fn wide_from_ptr(ptr: *const u16) -> Vec<u16> {
    if ptr.is_null() {
        return vec![0];
    }
    let mut len = 0;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    let mut value = std::slice::from_raw_parts(ptr, len).to_vec();
    value.push(0);
    value
}
