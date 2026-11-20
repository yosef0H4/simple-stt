use anyhow::{bail, Context, Result};
use clap::Parser;
use libloading::Library;
use reader_core::wire::{
    read_json_frame, write_json_frame, write_pcm_frame, PcmHeader, WorkerEvent, WorkerRequest,
};
use std::collections::{HashSet, VecDeque};
use std::ffi::{CStr, CString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone, Parser)]
#[command(name = "reader-tts", about = "Disposable native speech worker")]
struct Args {
    #[arg(long)]
    model_dir: PathBuf,
    #[arg(long)]
    runtime_dir: PathBuf,
    #[arg(long, default_value = "auto")]
    device: String,
    #[arg(long, default_value_t = 2)]
    threads: i32,
    #[arg(long, default_value = "en-us")]
    language: String,
    #[arg(long)]
    library: Option<PathBuf>,
    #[arg(long, default_value = "local_kokoro")]
    provider: String,
    #[arg(long)]
    base_url: Option<String>,
    #[arg(long)]
    model: Option<String>,
    #[arg(long)]
    remote_voice: Option<String>,
    #[arg(long, default_value = "wav")]
    format: String,
    #[arg(long, default_value_t = false, action = clap::ArgAction::Set)]
    fallback_to_local: bool,
    #[arg(long, default_value = "local_kokoro")]
    fallback_provider: String,
    #[arg(long, default_value_t = 3)]
    request_connect_timeout_secs: u64,
    #[arg(long, default_value_t = 30)]
    request_queue_timeout_secs: u64,
    #[arg(long, default_value_t = 180)]
    request_cold_model_load_timeout_secs: u64,
    #[arg(long, default_value_t = 30)]
    request_warm_model_load_timeout_secs: u64,
    #[arg(long, default_value_t = 60)]
    request_warm_inference_timeout_secs: u64,
    #[arg(long, default_value_t = 60)]
    request_stream_inactivity_timeout_secs: u64,
    #[arg(long, default_value_t = 300)]
    request_total_timeout_secs: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Vits {
    model: *const i8,
    lexicon: *const i8,
    tokens: *const i8,
    data_dir: *const i8,
    noise_scale: f32,
    noise_scale_w: f32,
    length_scale: f32,
    dict_dir: *const i8,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Matcha {
    acoustic_model: *const i8,
    vocoder: *const i8,
    lexicon: *const i8,
    tokens: *const i8,
    data_dir: *const i8,
    noise_scale: f32,
    length_scale: f32,
    dict_dir: *const i8,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Kokoro {
    model: *const i8,
    voices: *const i8,
    tokens: *const i8,
    data_dir: *const i8,
    length_scale: f32,
    dict_dir: *const i8,
    lexicon: *const i8,
    lang: *const i8,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Kitten {
    model: *const i8,
    voices: *const i8,
    tokens: *const i8,
    data_dir: *const i8,
    length_scale: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Zipvoice {
    tokens: *const i8,
    encoder: *const i8,
    decoder: *const i8,
    vocoder: *const i8,
    data_dir: *const i8,
    lexicon: *const i8,
    feat_scale: f32,
    t_shift: f32,
    target_rms: f32,
    guidance_scale: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Pocket {
    lm_flow: *const i8,
    lm_main: *const i8,
    encoder: *const i8,
    decoder: *const i8,
    text_conditioner: *const i8,
    vocab_json: *const i8,
    token_scores_json: *const i8,
    voice_embedding_cache_capacity: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Supertonic {
    duration_predictor: *const i8,
    text_encoder: *const i8,
    vector_estimator: *const i8,
    vocoder: *const i8,
    tts_json: *const i8,
    unicode_indexer: *const i8,
    voice_style: *const i8,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Model {
    vits: Vits,
    num_threads: i32,
    debug: i32,
    provider: *const i8,
    matcha: Matcha,
    kokoro: Kokoro,
    kitten: Kitten,
    zipvoice: Zipvoice,
    pocket: Pocket,
    supertonic: Supertonic,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TtsConfig {
    model: Model,
    rule_fsts: *const i8,
    max_num_sentences: i32,
    rule_fars: *const i8,
    silence_scale: f32,
}
#[repr(C)]
struct Audio {
    samples: *const f32,
    n: i32,
    sample_rate: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct GenerationConfig {
    silence_scale: f32,
    speed: f32,
    sid: i32,
    reference_audio: *const f32,
    reference_audio_len: i32,
    reference_sample_rate: i32,
    reference_text: *const i8,
    num_steps: i32,
    extra: *const i8,
}

struct NativeTts {
    _lib: Library,
    #[cfg(windows)]
    _onnx: Library,
    handle: *const std::ffi::c_void,
    generate: unsafe extern "C" fn(
        *const std::ffi::c_void,
        *const i8,
        *const GenerationConfig,
        Option<unsafe extern "C" fn(*const f32, i32, f32, *mut std::ffi::c_void) -> i32>,
        *mut std::ffi::c_void,
    ) -> *const Audio,
    destroy_audio: unsafe extern "C" fn(*const Audio),
    destroy: unsafe extern "C" fn(*const std::ffi::c_void),
    sample_rate: u32,
    supertonic: bool,
}
impl Drop for NativeTts {
    fn drop(&mut self) {
        unsafe { (self.destroy)(self.handle) }
    }
}

impl NativeTts {
    fn load(args: &Args, provider: &str) -> Result<Self> {
        let library_path = args.library.clone().unwrap_or_else(|| {
            if cfg!(windows) {
                [
                    args.runtime_dir.join("bin/sherpa-onnx-c-api.dll"),
                    args.runtime_dir.join("lib/sherpa-onnx-c-api.dll"),
                ]
                .into_iter()
                .find(|path| path.is_file())
                .unwrap_or_else(|| args.runtime_dir.join("bin/sherpa-onnx-c-api.dll"))
            } else {
                args.runtime_dir.join("lib/libsherpa-onnx-c-api.so")
            }
        });
        #[cfg(windows)]
        let onnx = unsafe {
            Library::new(
                library_path
                    .parent()
                    .context("Sherpa library has no parent")?
                    .join("onnxruntime.dll"),
            )
        }
        .context("loading the selected Windows ONNX runtime")?;
        #[cfg(windows)]
        let library = unsafe {
            // Resolve ONNX dependencies beside the selected runtime, before System32.
            const SEARCH_DLL_LOAD_DIR: u32 = 0x00000100;
            const SEARCH_DEFAULT_DIRS: u32 = 0x00001000;
            libloading::os::windows::Library::load_with_flags(
                &library_path,
                SEARCH_DLL_LOAD_DIR | SEARCH_DEFAULT_DIRS,
            )
            .map(Library::from)
        };
        #[cfg(not(windows))]
        let library = unsafe { Library::new(&library_path) };
        let library = library
            .with_context(|| format!("loading Sherpa-ONNX C API at {}", library_path.display()))?;
        let version = unsafe {
            library.get::<unsafe extern "C" fn() -> *const i8>(b"SherpaOnnxGetVersionStr\0")
        }
        .context("Sherpa library has no version function")?;
        let version = unsafe { CStr::from_ptr(version()) }.to_string_lossy();
        if !version.contains("1.13.8") {
            bail!("unsupported Sherpa-ONNX ABI version {version}; expected v1.13.8");
        }
        let create = unsafe {
            library.get::<unsafe extern "C" fn(*const TtsConfig) -> *const std::ffi::c_void>(
                b"SherpaOnnxCreateOfflineTts\0",
            )
        }
        .context("Sherpa C API is missing TTS constructor")?;
        let destroy = unsafe {
            *library.get::<unsafe extern "C" fn(*const std::ffi::c_void)>(
                b"SherpaOnnxDestroyOfflineTts\0",
            )?
        };
        let sample_rate_fn = unsafe {
            library.get::<unsafe extern "C" fn(*const std::ffi::c_void) -> i32>(
                b"SherpaOnnxOfflineTtsSampleRate\0",
            )
        }
        .context("Sherpa C API is missing sample-rate query")?;
        let generate = *unsafe {
            library.get::<unsafe extern "C" fn(
                *const std::ffi::c_void,
                *const i8,
                *const GenerationConfig,
                Option<unsafe extern "C" fn(*const f32, i32, f32, *mut std::ffi::c_void) -> i32>,
                *mut std::ffi::c_void,
            ) -> *const Audio>(b"SherpaOnnxOfflineTtsGenerateWithConfig\0")
        }
        .context("Sherpa C API is missing TTS generator")?;
        let destroy_audio = unsafe {
            *library.get::<unsafe extern "C" fn(*const Audio)>(
                b"SherpaOnnxDestroyOfflineTtsGeneratedAudio\0",
            )?
        };
        let model = args
            .model_dir
            .canonicalize()
            .context("Kokoro model directory is unavailable")?;
        let supertonic = args.provider == "local_supertonic";
        let provider = CString::new(provider)?;
        let mut config = TtsConfig::default();
        config.model.num_threads = args.threads.clamp(1, 8);
        config.model.provider = provider.as_ptr();
        // Keep every C string alive through the constructor call.
        let paths: Vec<CString> = if supertonic {
            [
                "duration_predictor.int8.onnx",
                "text_encoder.int8.onnx",
                "vector_estimator.int8.onnx",
                "vocoder.int8.onnx",
                "tts.json",
                "unicode_indexer.bin",
                "voice.bin",
            ]
            .iter()
            .map(|name| find_file(&model, &[*name]).and_then(|path| cstr_path(&path)))
            .collect::<Result<_>>()?
        } else {
            [
                "model.onnx",
                "voices.bin",
                "tokens.txt",
                "espeak-ng-data",
                if args.language.starts_with("en-gb") {
                    "lexicon-gb-en.txt"
                } else {
                    "lexicon-us-en.txt"
                },
            ]
            .iter()
            .map(|name| cstr_path(&model.join(name)))
            .collect::<Result<_>>()?
        };
        let lang = CString::new(args.language.as_str())?;
        if supertonic {
            config.model.supertonic = Supertonic {
                duration_predictor: paths[0].as_ptr(),
                text_encoder: paths[1].as_ptr(),
                vector_estimator: paths[2].as_ptr(),
                vocoder: paths[3].as_ptr(),
                tts_json: paths[4].as_ptr(),
                unicode_indexer: paths[5].as_ptr(),
                voice_style: paths[6].as_ptr(),
            };
        } else {
            // Validate required files before passing them to native code.
            for name in ["model.onnx", "voices.bin", "tokens.txt"] {
                find_file(&model, &[name])?;
            }
            config.model.kokoro = Kokoro {
                model: paths[0].as_ptr(),
                voices: paths[1].as_ptr(),
                tokens: paths[2].as_ptr(),
                data_dir: paths[3].as_ptr(),
                length_scale: 1.0,
                dict_dir: ptr::null(),
                lexicon: paths[4].as_ptr(),
                lang: lang.as_ptr(),
            };
        }
        config.max_num_sentences = 1;
        let handle = unsafe { create(&config) };
        if handle.is_null() {
            bail!("Sherpa-ONNX could not load the Kokoro model with provider {provider:?}");
        }
        let sample_rate = unsafe { sample_rate_fn(handle) };
        if !(8_000..=192_000).contains(&sample_rate) {
            unsafe { destroy(handle) };
            bail!("Sherpa returned an invalid sample rate {sample_rate}");
        }
        Ok(Self {
            _lib: library,
            #[cfg(windows)]
            _onnx: onnx,
            handle,
            generate,
            destroy_audio,
            destroy,
            sample_rate: sample_rate as u32,
            supertonic,
        })
    }

    fn synthesize(
        &self,
        text: &str,
        voice: &str,
        speed: f32,
        language: &str,
        steps: Option<u32>,
        cancelled: &AtomicBool,
    ) -> Result<Option<Vec<i16>>> {
        let sid = if self.supertonic {
            voice
                .strip_prefix("sid:")
                .unwrap_or(voice)
                .parse::<i32>()
                .ok()
                .filter(|id| (0..10).contains(id))
                .with_context(|| {
                    format!(
                        "unsupported Supertonic speaker '{voice}'; expected sid:0 through sid:9"
                    )
                })?
        } else {
            voice_id(voice).with_context(|| format!("unsupported Kokoro voice '{voice}'"))?
        };
        let extra = if self.supertonic {
            let code = language
                .split(['-', '_'])
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            anyhow::ensure!(
                [
                    "en", "ko", "ja", "ar", "bg", "cs", "da", "de", "el", "es", "et", "fi", "fr",
                    "hi", "hr", "hu", "id", "it", "lt", "lv", "nl", "pl", "pt", "ro", "ru", "sk",
                    "sl", "sv", "tr", "uk", "vi"
                ]
                .contains(&code.as_str()),
                "unsupported Supertonic language '{language}'"
            );
            Some(CString::new(serde_json::json!({"lang": code}).to_string())?)
        } else {
            None
        };
        let text = CString::new(text).context("speech input contains a null character")?;
        let config = GenerationConfig {
            speed,
            sid,
            silence_scale: 0.15,
            num_steps: generation_steps(steps),
            extra: extra.as_ref().map_or(ptr::null(), |value| value.as_ptr()),
            ..GenerationConfig::default()
        };
        let generated = unsafe {
            (self.generate)(
                self.handle,
                text.as_ptr(),
                &config,
                Some(cancel_progress),
                cancelled as *const AtomicBool as *mut std::ffi::c_void,
            )
        };
        if generated.is_null() {
            if cancelled.load(Ordering::Acquire) {
                return Ok(None);
            }
            bail!("Kokoro synthesis failed")
        }
        let audio = unsafe { &*generated };
        if cancelled.load(Ordering::Acquire) {
            unsafe { (self.destroy_audio)(generated) };
            return Ok(None);
        }
        if audio.n <= 0 || audio.n as usize > self.sample_rate as usize * 30 {
            unsafe { (self.destroy_audio)(generated) };
            bail!("Kokoro returned empty or overlong audio")
        }
        let output = unsafe { std::slice::from_raw_parts(audio.samples, audio.n as usize) }
            .iter()
            .map(|x| (x.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            .collect();
        unsafe { (self.destroy_audio)(generated) };
        Ok(Some(output))
    }
}

unsafe extern "C" fn cancel_progress(
    _samples: *const f32,
    _count: i32,
    _progress: f32,
    context: *mut std::ffi::c_void,
) -> i32 {
    let cancelled = unsafe { &*(context.cast::<AtomicBool>()) };
    i32::from(!cancelled.load(Ordering::Acquire))
}

fn find_file(dir: &Path, names: &[&str]) -> Result<PathBuf> {
    for name in names {
        let path = dir.join(name);
        if path.is_file() {
            return Ok(path);
        }
    }
    bail!("model pack is missing one of {}", names.join(", "))
}
fn cstr_path(path: &Path) -> Result<CString> {
    let text = path.to_string_lossy();
    #[cfg(windows)]
    let text = if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
    };
    CString::new(text.as_bytes()).context("model path contains a null byte")
}
fn voice_id(value: &str) -> Option<i32> {
    Some(match value {
        "af_heart" => 3,
        "af_bella" => 2,
        "af_nicole" => 6,
        "af_sarah" => 9,
        "am_adam" => 11,
        "am_michael" => 16,
        "bf_emma" => 21,
        _ => return None,
    })
}
fn generation_steps(steps: Option<u32>) -> i32 {
    steps
        .filter(|steps| *steps > 0)
        .unwrap_or(5)
        .min(i32::MAX as u32) as i32
}

fn request_budgets(args: &Args) -> reader_core::request_budget::RequestBudgets {
    use std::time::Duration;
    reader_core::request_budget::RequestBudgets {
        connect: Duration::from_secs(args.request_connect_timeout_secs),
        queue: Duration::from_secs(args.request_queue_timeout_secs),
        cold_model_load: Duration::from_secs(args.request_cold_model_load_timeout_secs),
        warm_model_load: Duration::from_secs(args.request_warm_model_load_timeout_secs),
        warm_inference: Duration::from_secs(args.request_warm_inference_timeout_secs),
        stream_inactivity: Duration::from_secs(args.request_stream_inactivity_timeout_secs),
        total: Duration::from_secs(args.request_total_timeout_secs),
    }
}

#[derive(Debug)]
struct TransientRemoteError(String);

impl std::fmt::Display for TransientRemoteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for TransientRemoteError {}

fn transient_remote_error(message: String) -> anyhow::Error {
    TransientRemoteError(message).into()
}

fn is_transient_remote_error(error: &anyhow::Error) -> bool {
    error.downcast_ref::<TransientRemoteError>().is_some()
}

fn run_local_fallback<T>(
    remote_error: anyhow::Error,
    enabled: bool,
    local: impl FnOnce() -> Result<T>,
) -> Result<T> {
    if enabled && is_transient_remote_error(&remote_error) {
        local().map_err(|local_error| {
            anyhow::anyhow!("remote speech failed ({remote_error}); local fallback unavailable or failed: {local_error}. Install the configured local speech runtime and model, or disable local fallback.")
        })
    } else {
        Err(remote_error)
    }
}

fn remote_stream(
    args: &Args,
    transport: (&reqwest::blocking::Client, std::time::Duration),
    text: &str,
    voice: &str,
    speed: f32,
    cancelled: &AtomicBool,
    mut emit: impl FnMut(&[i16], u32, u64, bool) -> Result<()>,
) -> Result<(u32, u64)> {
    let (client, request_timeout) = transport;
    anyhow::ensure!(
        args.format == "wav" || args.format == "pcm",
        "remote speech response format must be WAV or PCM"
    );
    let base = args
        .base_url
        .as_deref()
        .context("remote speech needs --base-url")?;
    let model = args
        .model
        .as_deref()
        .context("remote speech needs --model")?;
    let key = std::env::var("SIMPLE_TTS_SPEECH_API_KEY").unwrap_or_default();
    let mut request = client
        .post(reader_core::provider_url::operation(base, "audio/speech")?)
        .json(&serde_json::json!({
            "model": model,
            "input": text,
            "voice": args.remote_voice.as_deref().unwrap_or(voice),
            "response_format": args.format,
            "speed": speed
        }));
    if !key.is_empty() {
        request = request.bearer_auth(&key)
    }
    let response = request.timeout(request_timeout).send().map_err(|error| {
        if error.is_timeout() || error.is_connect() {
            transient_remote_error(format!("connecting to remote speech provider: {error}"))
        } else {
            anyhow::Error::from(error).context("connecting to remote speech provider")
        }
    })?;
    let status = response.status();
    if !status.is_success() {
        let mut error_body = Vec::new();
        use std::io::Read;
        response
            .take(65_536)
            .read_to_end(&mut error_body)
            .context("reading speech provider rejection")?;
        let message = reader_core::provider_error::http_error(
            "speech provider",
            &status.to_string(),
            &error_body,
            &key,
        );
        if status.as_u16() == 429 || status.is_server_error() {
            return Err(transient_remote_error(message));
        }
        bail!(message)
    }
    let mut reader = response.take(64 * 1024 * 1024);
    let (rate, data_size) = if args.format == "pcm" {
        (24_000u32, None)
    } else {
        let mut riff = [0u8; 12];
        reader
            .read_exact(&mut riff)
            .map_err(|error| transient_io_error("speech response has no WAV header", error))?;
        anyhow::ensure!(
            &riff[0..4] == b"RIFF" && &riff[8..12] == b"WAVE",
            "speech provider response is not a RIFF WAV stream"
        );
        let mut rate = 0;
        let mut format = 0;
        let mut channels = 0;
        let mut bits = 0;
        let data_len = loop {
            let mut chunk = [0u8; 8];
            reader
                .read_exact(&mut chunk)
                .context("truncated WAV header")?;
            let len = u32::from_le_bytes(chunk[4..8].try_into().unwrap());
            if &chunk[0..4] == b"fmt " {
                anyhow::ensure!((16..=4096).contains(&len), "unsupported WAV format block");
                let mut fmt = vec![0u8; len as usize];
                reader.read_exact(&mut fmt)?;
                format = u16::from_le_bytes([fmt[0], fmt[1]]);
                channels = u16::from_le_bytes([fmt[2], fmt[3]]);
                rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
                bits = u16::from_le_bytes([fmt[14], fmt[15]]);
            } else if &chunk[0..4] == b"data" {
                break len;
            } else {
                let mut skip = reader.by_ref().take(len as u64);
                std::io::copy(&mut skip, &mut std::io::sink())?;
                if len % 2 == 1 {
                    let mut pad = [0];
                    reader.read_exact(&mut pad)?;
                }
            }
        };
        anyhow::ensure!(
            format == 1 && channels == 1 && bits == 16 && (8_000..=192_000).contains(&rate),
            "speech WAV must be mono 16-bit PCM with a supported sample rate"
        );
        (rate, Some(data_len))
    };
    let mut pending = Vec::<i16>::with_capacity(8192);
    let mut offset = 0u64;
    let mut total = 0u64;
    let mut raw = [0u8; 8192];
    let mut pending_byte = None;
    let mut remaining = data_size;
    loop {
        anyhow::ensure!(
            !cancelled.load(Ordering::Acquire),
            "remote speech synthesis canceled"
        );
        let max = remaining
            .map(|n| n.min(raw.len() as u32) as usize)
            .unwrap_or(raw.len());
        if max == 0 {
            break;
        }
        let n = reader
            .read(&mut raw[..max])
            .map_err(|error| transient_io_error("reading streamed speech audio", error))?;
        if n == 0 {
            break;
        }
        if let Some(value) = remaining.as_mut() {
            *value -= n as u32
        }
        let mut i = 0;
        if let Some(low) = pending_byte.take() {
            if n > 0 {
                pending.push(i16::from_le_bytes([low, raw[0]]));
                i = 1
            } else {
                pending_byte = Some(low)
            }
        }
        while i + 1 < n {
            pending.push(i16::from_le_bytes([raw[i], raw[i + 1]]));
            i += 2
        }
        if i < n {
            pending_byte = Some(raw[i]);
        }
        while pending.len() > 4096 {
            let packet: Vec<i16> = pending.drain(..4096).collect();
            total += packet.len() as u64;
            anyhow::ensure!(
                total <= rate as u64 * 30,
                "speech provider audio exceeds the 30-second unit limit"
            );
            emit(&packet, rate, offset, false)?;
            offset = total;
        }
        if remaining == Some(0) {
            break;
        }
    }
    anyhow::ensure!(
        pending_byte.is_none(),
        "speech provider returned odd-length PCM data"
    );
    if !pending.is_empty() {
        total += pending.len() as u64;
        anyhow::ensure!(
            total <= rate as u64 * 30,
            "speech provider audio exceeds the 30-second unit limit"
        );
        emit(&pending, rate, offset, true)?
    }
    anyhow::ensure!(total > 0, "speech provider returned no audio");
    Ok((rate, total))
}

fn transient_io_error(context: &str, error: std::io::Error) -> anyhow::Error {
    use std::io::ErrorKind;
    if matches!(
        error.kind(),
        ErrorKind::TimedOut
            | ErrorKind::Interrupted
            | ErrorKind::UnexpectedEof
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::BrokenPipe
    ) {
        transient_remote_error(format!("{context}: {error}"))
    } else {
        anyhow::Error::from(error).context(context.to_owned())
    }
}

fn remote_stream_with_retry(
    args: &Args,
    client: &reqwest::blocking::Client,
    text: &str,
    voice: &str,
    speed: f32,
    cancelled: &AtomicBool,
    mut emit: impl FnMut(&[i16], u32, u64, bool) -> Result<()>,
) -> Result<(u32, u64)> {
    use reader_core::request_budget::{RequestBudget, RequestStage};
    let budgets = request_budgets(args);
    let started = std::time::Instant::now();
    let mut budget = RequestBudget::new(started, true, budgets);
    budget.set_stage(RequestStage::LoadingModel, started);
    for attempt in 0..2 {
        anyhow::ensure!(
            !cancelled.load(Ordering::Acquire),
            "remote speech synthesis canceled"
        );
        let remaining = budget
            .deadline()
            .unwrap_or(started)
            .saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(transient_remote_error(
                "speech request exceeded its configured timeout budget".into(),
            ));
        }
        let mut buffered = Vec::<(Vec<i16>, u32, u64, bool)>::new();
        let result = remote_stream(
            args,
            (client, remaining),
            text,
            voice,
            speed,
            cancelled,
            |samples, rate, offset, final_chunk| {
                buffered.push((samples.to_vec(), rate, offset, final_chunk));
                Ok(())
            },
        );
        match result {
            Ok(total) => {
                for (samples, rate, offset, final_chunk) in buffered {
                    emit(&samples, rate, offset, final_chunk)?;
                }
                return Ok(total);
            }
            Err(error) if attempt == 0 && is_transient_remote_error(&error) => {
                tracing::warn!(%error, "transient speech-provider failure; retrying once");
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(error) => return Err(error),
        }
    }
    Err(transient_remote_error(
        "speech provider remained unavailable after bounded retry".into(),
    ))
}
fn emit(event: &WorkerEvent) -> Result<()> {
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    write_json_frame(&mut lock, event)
}

#[derive(Default)]
struct EngineState {
    engine: Option<NativeTts>,
    provider: String,
    cuda_proven: bool,
    device: String,
}
struct SynthesisRequest<'a> {
    job_id: &'a str,
    generation: u64,
    sentence: u32,
    unit: u32,
    text: &'a str,
    voice: &'a str,
    speed: f32,
}
struct LocalSynthesisRequest<'a> {
    text: &'a str,
    voice: &'a str,
    language: &'a str,
    speed: f32,
    steps: Option<u32>,
    device: &'a str,
}

fn process_request(
    args: &Args,
    client: &reqwest::blocking::Client,
    state: &mut EngineState,
    cancelled: &AtomicBool,
    request: WorkerRequest,
) -> Result<bool> {
    match request {
        WorkerRequest::Shutdown => Ok(true),
        WorkerRequest::Hello {
            protocol,
            incarnation,
        } => {
            anyhow::ensure!(protocol == 1, "unsupported controller protocol {protocol}");
            emit(&WorkerEvent::Ready {
                incarnation,
                engine: engine_label(args).into(),
                device: "Worker ready; model loading is deferred until requested".into(),
            })?;
            Ok(false)
        }
        WorkerRequest::Prepare { device } => {
            prepare(args, state, &device)?;
            Ok(false)
        }
        WorkerRequest::Synthesize {
            job_id,
            generation,
            sentence,
            unit,
            text,
            voice,
            steps,
            language,
            speed,
            device,
        } => {
            if matches!(args.provider.as_str(), "local_kokoro" | "local_supertonic") {
                let result = (|| {
                    if state.engine.is_none() {
                        prepare(args, state, &device)?;
                    }
                    synthesize_local(
                        args,
                        state,
                        LocalSynthesisRequest {
                            text: &text,
                            voice: &voice,
                            language: &language,
                            speed,
                            steps,
                            device: &device,
                        },
                        cancelled,
                    )
                })();
                if cancelled.load(Ordering::Acquire) {
                    if state.engine.is_some() {
                        emit_local_ready(args, &job_id, state)?;
                    }
                    emit(&WorkerEvent::Cancelled { job_id, generation })?;
                    return Ok(false);
                }
                let Some(samples) = result? else {
                    if state.engine.is_some() {
                        emit_local_ready(args, &job_id, state)?;
                    }
                    emit(&WorkerEvent::Cancelled { job_id, generation })?;
                    return Ok(false);
                };
                emit_local_ready(args, &job_id, state)?;
                if !emit_local_audio(
                    &job_id, generation, sentence, unit, state, &samples, cancelled,
                )? {
                    emit(&WorkerEvent::Cancelled { job_id, generation })?;
                    return Ok(false);
                }
            } else {
                let result = synthesize_remote(
                    args,
                    client,
                    SynthesisRequest {
                        job_id: &job_id,
                        generation,
                        sentence,
                        unit,
                        text: &text,
                        voice: &voice,
                        speed,
                    },
                    cancelled,
                );
                if cancelled.load(Ordering::Acquire) {
                    emit(&WorkerEvent::Cancelled { job_id, generation })?;
                    return Ok(false);
                }
                if let Err(remote_error) = result {
                    if cancelled.load(Ordering::Acquire) {
                        emit(&WorkerEvent::Cancelled { job_id, generation })?;
                        return Ok(false);
                    }
                    if args.fallback_to_local && is_transient_remote_error(&remote_error) {
                        let local_args = Args {
                            provider: args.fallback_provider.clone(),
                            ..args.clone()
                        };
                        if !matches!(
                            local_args.provider.as_str(),
                            "local_kokoro" | "local_supertonic"
                        ) {
                            bail!(
                                "local speech fallback provider must be local_kokoro or local_supertonic"
                            )
                        }
                        let local_result = run_local_fallback(remote_error, true, || {
                            synthesize_local(
                                &local_args,
                                state,
                                LocalSynthesisRequest {
                                    text: &text,
                                    voice: &voice,
                                    language: &language,
                                    speed,
                                    steps,
                                    device: &device,
                                },
                                cancelled,
                            )
                        });
                        match local_result {
                            Ok(Some(samples)) => {
                                emit(&WorkerEvent::Ready {
                                    incarnation: job_id.clone(),
                                    engine: format!(
                                        "{} (local fallback)",
                                        engine_label(&local_args)
                                    ),
                                    device: "Remote speech failed; using installed local model"
                                        .into(),
                                })?;
                                if !emit_local_audio(
                                    &job_id, generation, sentence, unit, state, &samples, cancelled,
                                )? {
                                    emit(&WorkerEvent::Cancelled { job_id, generation })?;
                                    return Ok(false);
                                }
                            }
                            Ok(None) => {
                                emit(&WorkerEvent::Cancelled { job_id, generation })?;
                                return Ok(false);
                            }
                            Err(local_error) => {
                                emit(&WorkerEvent::Error {
                                    job_id: Some(job_id),
                                    generation: Some(generation),
                                    code: "speech_fallback_failed".into(),
                                    message: format!("{local_error:#}"),
                                })?;
                                return Ok(false);
                            }
                        }
                    } else {
                        let message = format!("{remote_error:#}");
                        emit(&WorkerEvent::Error {
                            job_id: Some(job_id),
                            generation: Some(generation),
                            code: "remote_speech_failed".into(),
                            message,
                        })?;
                        return Ok(false);
                    }
                }
            }
            emit(&WorkerEvent::Finished { job_id, generation })?;
            Ok(false)
        }
        WorkerRequest::Cancel { .. } => Ok(false),
        WorkerRequest::Recognize { .. } => {
            emit(&WorkerEvent::Error {
                job_id: None,
                generation: None,
                code: "wrong_worker".into(),
                message: "OCR requests must be sent to reader-ocr".into(),
            })?;
            Ok(false)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct JobKey {
    id: String,
    generation: u64,
}

#[derive(Default)]
struct WorkerControl {
    active: Option<JobKey>,
    queued: HashSet<JobKey>,
    canceled: HashSet<JobKey>,
    completed: HashSet<JobKey>,
    completed_order: VecDeque<JobKey>,
}

enum InputMessage {
    Request(WorkerRequest),
    Error(String),
}

fn request_job_key(request: &WorkerRequest) -> Option<JobKey> {
    match request {
        WorkerRequest::Synthesize {
            job_id, generation, ..
        } => Some(JobKey {
            id: job_id.clone(),
            generation: *generation,
        }),
        _ => None,
    }
}

fn start_control_reader(
    control: Arc<Mutex<WorkerControl>>,
    cancelled: Arc<AtomicBool>,
) -> Receiver<InputMessage> {
    let (sender, receiver) = sync_channel(8);
    std::thread::spawn(move || read_control_input(sender, control, cancelled));
    receiver
}

fn read_control_input(
    sender: SyncSender<InputMessage>,
    control: Arc<Mutex<WorkerControl>>,
    cancelled: Arc<AtomicBool>,
) {
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    loop {
        let request = match read_json_frame::<WorkerRequest>(&mut reader) {
            Ok(request) => request,
            Err(error) if error.to_string().contains("failed to fill whole buffer") => break,
            Err(error) => {
                let _ = sender.send(InputMessage::Error(error.to_string()));
                break;
            }
        };
        match request {
            WorkerRequest::Cancel { job_id, generation } => {
                let key = JobKey {
                    id: job_id,
                    generation,
                };
                let mut state = control
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                record_cancel(key, &mut state, &cancelled);
            }
            request => {
                if let Some(key) = request_job_key(&request) {
                    control
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .queued
                        .insert(key);
                }
                if sender.send(InputMessage::Request(request)).is_err() {
                    break;
                }
            }
        }
    }
}

fn record_cancel(key: JobKey, state: &mut WorkerControl, cancelled: &AtomicBool) {
    if state.active.as_ref() == Some(&key) {
        cancelled.store(true, Ordering::Release);
    } else if state.queued.contains(&key) || !state.completed.contains(&key) {
        state.canceled.insert(key);
        while state.canceled.len() > 256 {
            let Some(oldest) = state.canceled.iter().next().cloned() else {
                break;
            };
            state.canceled.remove(&oldest);
        }
    }
}

fn activate_request(
    request: &WorkerRequest,
    control: &Mutex<WorkerControl>,
    cancelled: &AtomicBool,
) {
    let Some(key) = request_job_key(request) else {
        return;
    };
    let mut state = control
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.queued.remove(&key);
    let was_canceled = state.canceled.remove(&key);
    state.completed.remove(&key);
    state.completed_order.retain(|completed| completed != &key);
    state.active = Some(key);
    cancelled.store(was_canceled, Ordering::Release);
}

fn finish_request(request: &WorkerRequest, control: &Mutex<WorkerControl>, cancelled: &AtomicBool) {
    let Some(key) = request_job_key(request) else {
        return;
    };
    let mut state = control
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if state.active.as_ref() == Some(&key) {
        state.active = None;
    }
    state.completed.insert(key.clone());
    state.completed_order.push_back(key);
    while state.completed_order.len() > 256 {
        if let Some(expired) = state.completed_order.pop_front() {
            state.completed.remove(&expired);
        }
    }
    cancelled.store(false, Ordering::Release);
}

fn prepare(args: &Args, state: &mut EngineState, device: &str) -> Result<()> {
    if !matches!(args.provider.as_str(), "local_kokoro" | "local_supertonic") {
        return emit(&WorkerEvent::Ready {
            incarnation: "prepared".into(),
            engine: "OpenAI-compatible speech".into(),
            device: "Remote route configured; provider contact occurs on request".into(),
        });
    }
    if state.engine.is_none() {
        load_engine(args, state, device)?;
    }
    emit(&WorkerEvent::Ready {
        incarnation: "prepared".into(),
        engine: engine_label(args).into(),
        device: state.device.clone(),
    })
}

fn load_engine(args: &Args, state: &mut EngineState, device: &str) -> Result<()> {
    let prefer_cuda = matches!(device, "cuda" | "auto");
    match NativeTts::load(args, if prefer_cuda { "cuda" } else { "cpu" }) {
        Ok(loaded) => {
            state.engine = Some(loaded);
            state.provider = if prefer_cuda { "cuda" } else { "cpu" }.into();
            state.device = if prefer_cuda {
                "Model loaded · CUDA inference pending"
            } else {
                "CPU model loaded"
            }
            .into();
        }
        Err(error) if prefer_cuda && device == "auto" => {
            tracing::warn!(error=%error,"CUDA model setup failed; selecting CPU");
            state.engine = Some(NativeTts::load(args, "cpu")?);
            state.provider = "cpu".into();
            state.device = "CPU model loaded · CUDA setup failed".into();
        }
        Err(error) => return Err(error),
    }
    Ok(())
}

fn synthesize_local(
    args: &Args,
    state: &mut EngineState,
    request: LocalSynthesisRequest<'_>,
    cancelled: &AtomicBool,
) -> Result<Option<Vec<i16>>> {
    let prefer_cuda = matches!(request.device, "cuda" | "auto");
    if state.engine.is_none() {
        load_engine(args, state, request.device)?;
    }
    let mut samples = match state.engine.as_ref().unwrap().synthesize(
        request.text,
        request.voice,
        request.speed,
        request.language,
        request.steps,
        cancelled,
    ) {
        Ok(None) => return Ok(None),
        Ok(Some(audio)) => audio,
        Err(error) if prefer_cuda && request.device == "auto" => {
            if cancelled.load(Ordering::Acquire) {
                return Ok(None);
            }
            tracing::warn!(error=%error,"CUDA inference failed; recreating the worker model on CPU");
            state.engine.take();
            state.engine = Some(NativeTts::load(args, "cpu")?);
            state.provider = "cpu".into();
            state.cuda_proven = false;
            state.device = "CPU (CUDA inference failed)".into();
            state
                .engine
                .as_ref()
                .unwrap()
                .synthesize(
                    request.text,
                    request.voice,
                    request.speed,
                    request.language,
                    request.steps,
                    cancelled,
                )?
                .context("Kokoro synthesis was canceled")?
        }
        Err(error) => return Err(error),
    };
    if cancelled.load(Ordering::Acquire) {
        return Ok(None);
    }
    verify_cuda_or_fallback(args, state, &request, cancelled, &mut samples)?;
    Ok(Some(samples))
}

fn verify_cuda_or_fallback(
    args: &Args,
    state: &mut EngineState,
    request: &LocalSynthesisRequest<'_>,
    cancelled: &AtomicBool,
    samples: &mut Vec<i16>,
) -> Result<()> {
    if !matches!(request.device, "cuda" | "auto") || state.provider != "cuda" || state.cuda_proven {
        return Ok(());
    }
    if reader_core::cuda::onnx_cuda_verified() {
        state.cuda_proven = true;
        state.device = "CUDA verified · Sherpa provider + NVIDIA allocation".into();
        return Ok(());
    }
    if request.device != "auto" {
        anyhow::bail!(
            "CUDA was requested but this worker could not prove its own NVIDIA allocation after inference"
        );
    }
    tracing::warn!("CUDA provider did not prove an allocation for this worker; switching to CPU");
    state.engine.take();
    state.engine = Some(NativeTts::load(args, "cpu")?);
    state.provider = "cpu".into();
    state.device = "CPU (CUDA execution not proven)".into();
    *samples = state
        .engine
        .as_ref()
        .unwrap()
        .synthesize(
            request.text,
            request.voice,
            request.speed,
            request.language,
            request.steps,
            cancelled,
        )?
        .context("Kokoro synthesis was canceled")?;
    Ok(())
}

fn emit_local_audio(
    job_id: &str,
    generation: u64,
    sentence: u32,
    unit: u32,
    state: &EngineState,
    samples: &[i16],
    cancelled: &AtomicBool,
) -> Result<bool> {
    const CHUNK: usize = 4096;
    let total = samples.len();
    let rate = state.engine.as_ref().unwrap().sample_rate;
    for (index, chunk) in samples.chunks(CHUNK).enumerate() {
        if cancelled.load(Ordering::Acquire) {
            return Ok(false);
        }
        let final_chunk = (index + 1) * CHUNK >= total;
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        write_pcm_frame(
            &mut lock,
            &PcmHeader {
                job_id: job_id.into(),
                generation,
                sentence,
                unit,
                sample_rate: rate,
                sample_offset: (index * CHUNK) as u64,
                final_chunk,
                sample_count: chunk.len() as u32,
            },
            chunk,
        )?;
    }
    Ok(true)
}

fn engine_label(args: &Args) -> &'static str {
    if args.provider == "local_supertonic" {
        "sherpa-onnx-supertonic-v1.13.8"
    } else {
        "sherpa-onnx-kokoro-v1.13.8"
    }
}

fn emit_local_ready(args: &Args, job_id: &str, state: &EngineState) -> Result<()> {
    emit(&WorkerEvent::Ready {
        incarnation: job_id.into(),
        engine: engine_label(args).into(),
        device: state.device.clone(),
    })
}

fn synthesize_remote(
    args: &Args,
    client: &reqwest::blocking::Client,
    request: SynthesisRequest<'_>,
    cancelled: &AtomicBool,
) -> Result<()> {
    emit(&WorkerEvent::Ready {
        incarnation: request.job_id.into(),
        engine: "OpenAI-compatible speech".into(),
        device: "Remote provider".into(),
    })?;
    remote_stream_with_retry(
        args,
        client,
        request.text,
        request.voice,
        request.speed,
        cancelled,
        |chunk, rate, offset, final_chunk| {
            let stdout = std::io::stdout();
            let mut lock = stdout.lock();
            write_pcm_frame(
                &mut lock,
                &PcmHeader {
                    job_id: request.job_id.into(),
                    generation: request.generation,
                    sentence: request.sentence,
                    unit: request.unit,
                    sample_rate: rate,
                    sample_offset: offset,
                    final_chunk,
                    sample_count: chunk.len() as u32,
                },
                chunk,
            )
        },
    )
    .map(|_| ())
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let args = Args::parse();
    let budgets = request_budgets(&args);
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(budgets.connect)
        .build()?;
    let mut state = EngineState::default();
    let control = Arc::new(Mutex::new(WorkerControl::default()));
    let cancelled = Arc::new(AtomicBool::new(false));
    let input = start_control_reader(Arc::clone(&control), Arc::clone(&cancelled));
    while let Ok(message) = input.recv() {
        let request = match message {
            InputMessage::Request(request) => request,
            InputMessage::Error(error) => bail!("reading worker request: {error}"),
        };
        activate_request(&request, &control, &cancelled);
        let stop = process_request(&args, &client, &mut state, &cancelled, request.clone());
        finish_request(&request, &control, &cancelled);
        if stop? {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn native_paths_accept_canonical_drive_and_unc_paths() {
        for (input, expected) in [
            (r"\\?\C:\models\kokoro", r"C:\models\kokoro"),
            (r"\\?\UNC\host\models\kokoro", r"\\host\models\kokoro"),
            (r"C:\models\kokoro", r"C:\models\kokoro"),
        ] {
            assert_eq!(
                super::cstr_path(std::path::Path::new(input))
                    .unwrap()
                    .to_str()
                    .unwrap(),
                expected
            );
        }
    }

    use super::*;
    #[test]
    fn supertonic_steps_use_explicit_value_or_sherpa_default() {
        assert_eq!(generation_steps(None), 5);
        assert_eq!(generation_steps(Some(0)), 5);
        assert_eq!(generation_steps(Some(12)), 12);
    }
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    fn provider_503() -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let head = String::from_utf8_lossy(&request);
                let length = head
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                let mut body = vec![0; length];
                stream.read_exact(&mut body).unwrap();
                stream
                    .write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .unwrap();
            }
        });
        (format!("http://{address}/v1"), server)
    }

    fn remote_args(base_url: String, fallback_to_local: bool) -> Args {
        Args {
            model_dir: PathBuf::new(),
            runtime_dir: PathBuf::new(),
            device: "cpu".into(),
            threads: 2,
            language: "en-us".into(),
            library: None,
            provider: "openai_compatible".into(),
            base_url: Some(base_url),
            model: Some("fixture-model".into()),
            remote_voice: Some("alloy".into()),
            format: "pcm".into(),
            fallback_to_local,
            fallback_provider: "local_kokoro".into(),
            request_connect_timeout_secs: 3,
            request_queue_timeout_secs: 30,
            request_cold_model_load_timeout_secs: 180,
            request_warm_model_load_timeout_secs: 30,
            request_warm_inference_timeout_secs: 60,
            request_stream_inactivity_timeout_secs: 60,
            request_total_timeout_secs: 300,
        }
    }

    fn remote_failure(args: &Args) -> anyhow::Result<()> {
        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(request_budgets(args).connect)
            .build()?;
        remote_stream_with_retry(
            args,
            &client,
            "fixture text",
            "alloy",
            1.0,
            &AtomicBool::new(false),
            |_, _, _, _| Ok(()),
        )
        .map(|_| ())
    }

    #[test]
    fn cancellation_before_queued_synthesis_is_consumed_but_late_cancel_is_ignored() {
        let control = Mutex::new(WorkerControl::default());
        let canceled = AtomicBool::new(false);
        let key = JobKey {
            id: "request-a".into(),
            generation: 7,
        };
        {
            let mut state = control.lock().unwrap();
            record_cancel(key.clone(), &mut state, &canceled);
            assert!(state.canceled.contains(&key));
        }
        let request = WorkerRequest::Synthesize {
            job_id: key.id.clone(),
            generation: key.generation,
            sentence: 0,
            unit: 0,
            text: "hello".into(),
            voice: "af_heart".into(),
            steps: None,
            language: "en-us".into(),
            speed: 1.0,
            device: "auto".into(),
        };
        activate_request(&request, &control, &canceled);
        assert!(canceled.load(Ordering::Acquire));
        finish_request(&request, &control, &canceled);
        assert!(!canceled.load(Ordering::Acquire));
        {
            let mut state = control.lock().unwrap();
            record_cancel(key.clone(), &mut state, &canceled);
            assert!(!state.canceled.contains(&key));
        }
        activate_request(&request, &control, &canceled);
        assert!(!canceled.load(Ordering::Acquire));
    }

    #[test]
    fn pcm_response_streams_before_provider_finishes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sent_tx, sent_rx) = mpsc::channel();
        let (server_tx, server_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut head = Vec::new();
            let mut byte = [0];
            while !head.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                head.push(byte[0]);
            }
            let headers = String::from_utf8(head).unwrap();
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            let mut request_body = vec![0; content_length];
            stream.read_exact(&mut request_body).unwrap();
            assert!(serde_json::from_slice::<serde_json::Value>(&request_body).is_ok());
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            let first = vec![0x11; 8194];
            write!(stream, "{:x}\r\n", first.len()).unwrap();
            stream.write_all(&first).unwrap();
            stream.write_all(b"\r\n").unwrap();
            stream.flush().unwrap();
            server_tx.send(()).unwrap();
            sent_rx
                .recv_timeout(std::time::Duration::from_secs(3))
                .expect("worker should yield first PCM before response end");
            let last = [0x22; 2];
            write!(stream, "{:x}\r\n", last.len()).unwrap();
            stream.write_all(&last).unwrap();
            stream.write_all(b"\r\n0\r\n\r\n").unwrap();
        });
        let args = Args {
            model_dir: PathBuf::new(),
            runtime_dir: PathBuf::new(),
            device: "cpu".into(),
            threads: 2,
            language: "en-us".into(),
            library: None,
            provider: "openai_compatible".into(),
            base_url: Some(format!("http://{address}/v1")),
            model: Some("mock".into()),
            remote_voice: Some("alloy".into()),
            format: "pcm".into(),
            fallback_to_local: false,
            fallback_provider: "local_kokoro".into(),
            request_connect_timeout_secs: 3,
            request_queue_timeout_secs: 30,
            request_cold_model_load_timeout_secs: 180,
            request_warm_model_load_timeout_secs: 30,
            request_warm_inference_timeout_secs: 60,
            request_stream_inactivity_timeout_secs: 60,
            request_total_timeout_secs: 300,
        };
        let client = reqwest::blocking::Client::new();
        let cancelled = AtomicBool::new(false);
        let mut first = false;
        let mut final_packet = false;
        let (_, total) = remote_stream(
            &args,
            (&client, std::time::Duration::from_secs(3)),
            "hello",
            "alloy",
            1.0,
            &cancelled,
            |samples, rate, offset, last| {
                assert_eq!(rate, 24_000);
                if !first {
                    assert_eq!(samples.len(), 4096);
                    assert_eq!(offset, 0);
                    first = true;
                    server_rx
                        .recv_timeout(std::time::Duration::from_secs(3))
                        .unwrap();
                    sent_tx.send(()).unwrap();
                } else {
                    final_packet = last;
                    assert_eq!(offset, 4096);
                }
                Ok(())
            },
        )
        .unwrap();
        server.join().unwrap();
        assert!(first);
        assert!(final_packet);
        assert_eq!(total, 4098);
    }

    #[test]
    fn repeated_503_uses_local_speech_only_when_opted_in() {
        let (base_url, server) = provider_503();
        let args = remote_args(base_url, true);
        let remote_error = remote_failure(&args).unwrap_err();
        assert!(is_transient_remote_error(&remote_error));
        let output = run_local_fallback(remote_error, args.fallback_to_local, || {
            Ok("local speech fixture")
        })
        .unwrap();
        server.join().unwrap();
        assert_eq!(output, "local speech fixture");
    }

    #[test]
    fn repeated_503_does_not_use_local_speech_when_fallback_is_disabled() {
        let (base_url, server) = provider_503();
        let args = remote_args(base_url, false);
        let remote_error = remote_failure(&args).unwrap_err();
        let local_called = std::cell::Cell::new(false);
        assert!(
            run_local_fallback(remote_error, args.fallback_to_local, || {
                local_called.set(true);
                Ok("must not run")
            })
            .is_err()
        );
        server.join().unwrap();
        assert!(!local_called.get());
    }
}
