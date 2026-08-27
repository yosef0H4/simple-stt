use anyhow::{Context, Result};
use clap::Parser;
use simple_stt::config::{InferenceDevice, LogLevel};
use simple_stt::infer::parakeet_native::ParakeetNative;
use simple_stt::infer::protocol::{read_frame, write_frame, Frame, MessageType};
use std::io::{stdin, stdout, BufReader};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "simple-stt-infer",
    about = "Disposable SimpleStt Parakeet inference worker"
)]
struct Args {
    #[arg(long)]
    runtime_dir: PathBuf,
    #[arg(long)]
    model_path: PathBuf,
    #[arg(long)]
    log_path: PathBuf,
    #[arg(long, value_enum, default_value = "normal")]
    log_level: LogLevel,
    #[arg(long, value_enum, default_value = "auto")]
    inference_device: InferenceDevice,
    #[arg(long, default_value_t = 180)]
    idle_timeout_secs: u64,
}

fn main() -> Result<()> {
    let args = Args::parse();
    simple_stt::logging::init_component("infer", &args.log_path, &args.log_level)?;
    apply_inference_device(&args.inference_device);
    tracing::info!(pid = std::process::id(), model = %args.model_path.display(), inference_device = args.inference_device.as_str(), "disposable inference worker started");
    let mut input = BufReader::new(stdin());
    let mut output = stdout();
    let mut engine: Option<ParakeetNative> = None;
    // The capture supervisor owns the idle policy. Keeping a second timer in
    // this child can unload the model during a long recording, before PCM is
    // sent for transcription.
    loop {
        let frame = match read_frame(&mut input) {
            Ok(frame) => frame,
            Err(error) => {
                tracing::warn!(%error, "worker input stream closed");
                break;
            }
        };
        match frame.kind {
            MessageType::Hello => write_frame(&mut output, &Frame::empty(MessageType::HelloAck))?,
            MessageType::Ping => write_frame(&mut output, &Frame::empty(MessageType::Pong))?,
            MessageType::WarmUp => {
                ensure_engine(&mut engine, &args)?;
                write_frame(&mut output, &Frame::empty(MessageType::ModelLoaded))?;
                tracing::info!("model warm-up begin");
                let silence = vec![0_i16; 1_600];
                let _ = engine.as_ref().unwrap().transcribe_pcm16_16k(&silence)?;
                tracing::info!("model warm-up end");
                write_frame(&mut output, &Frame::empty(MessageType::WarmUpAck))?;
            }
            MessageType::Shutdown => {
                tracing::info!("worker graceful shutdown requested");
                let _ = write_frame(&mut output, &Frame::empty(MessageType::ShutdownAck));
                break;
            }
            MessageType::TranscribePcm => {
                let session_id = frame.session_id;
                let response = (|| -> Result<String> {
                    let (sample_rate, samples) = frame.decode_pcm()?;
                    anyhow::ensure!(
                        sample_rate == 16_000,
                        "expected 16 kHz PCM, got {sample_rate}"
                    );
                    ensure_engine(&mut engine, &args)?;
                    tracing::info!(session_id, samples = samples.len(), "inference begin");
                    let transcript = engine.as_ref().unwrap().transcribe_pcm16_16k(&samples)?;
                    tracing::info!(
                        session_id,
                        transcript_chars = transcript.chars().count(),
                        "inference end"
                    );
                    Ok(transcript)
                })();
                write_result(&mut output, session_id, response)?;
            }
            MessageType::TranscribeWav => {
                let session_id = frame.session_id;
                let response = (|| -> Result<String> {
                    let path = PathBuf::from(frame.body_as_text()?);
                    ensure_engine(&mut engine, &args)?;
                    tracing::info!(session_id, audio = %path.display(), "WAV model test begin");
                    let transcript = engine.as_ref().unwrap().transcribe_wav(&path)?;
                    tracing::info!(
                        session_id,
                        transcript_chars = transcript.chars().count(),
                        "WAV model test end"
                    );
                    Ok(transcript)
                })();
                write_result(&mut output, session_id, response)?;
            }
            other => write_frame(
                &mut output,
                &Frame::text(
                    MessageType::Error,
                    frame.session_id,
                    format!("unexpected worker request: {other:?}"),
                ),
            )?,
        }
    }
    drop(engine);
    tracing::info!(
        pid = std::process::id(),
        "inference worker exiting; process exit is the memory cleanup guarantee"
    );
    #[cfg(target_os = "linux")]
    unsafe {
        // The Linux Parakeet runtime owns a process-global CUDA backend whose
        // C++ exit handler can run after the CUDA driver has begun unloading.
        // The per-model context is already freed above. Exit directly so the
        // kernel reclaims the remaining process-owned CUDA allocations without
        // invoking that unsafe native static destructor.
        libc::_exit(0);
    }
    #[cfg(not(target_os = "linux"))]
    return Ok(());
}

fn apply_inference_device(device: &InferenceDevice) {
    match device.effective() {
        InferenceDevice::Cpu => std::env::set_var("PARAKEET_DEVICE", "cpu"),
        InferenceDevice::NvidiaGpu => std::env::remove_var("PARAKEET_DEVICE"),
        InferenceDevice::Auto => unreachable!("auto must resolve before applying inference device"),
    }
}

fn ensure_engine(engine: &mut Option<ParakeetNative>, args: &Args) -> Result<()> {
    if engine.is_none() {
        tracing::info!(runtime = %args.runtime_dir.display(), model = %args.model_path.display(), "model load begin");
        *engine = Some(
            ParakeetNative::load(&args.runtime_dir, &args.model_path)
                .context("loading Parakeet model")?,
        );
        tracing::info!(model = %args.model_path.display(), "model load end");
    }
    Ok(())
}
fn write_result(
    output: &mut impl std::io::Write,
    session_id: u64,
    response: Result<String>,
) -> Result<()> {
    match response {
        Ok(text) => write_frame(
            output,
            &Frame::text(MessageType::Transcript, session_id, text),
        ),
        Err(error) => {
            tracing::error!(session_id, %error, "inference request failed");
            write_frame(
                output,
                &Frame::text(MessageType::Error, session_id, error.to_string()),
            )
        }
    }
}
