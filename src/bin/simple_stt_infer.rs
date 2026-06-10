use anyhow::{Context, Result};
use clap::Parser;
use simple_stt::config::LogLevel;
use simple_stt::infer::parakeet_native::ParakeetNative;
use simple_stt::infer::protocol::{read_frame, write_frame, Frame, MessageType};
use std::io::{stdin, stdout, BufReader};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

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
    #[arg(long, default_value_t = 180)]
    idle_timeout_secs: u64,
}

fn main() -> Result<()> {
    let args = Args::parse();
    simple_stt::logging::init_component("infer", &args.log_path, &args.log_level)?;
    tracing::info!(pid = std::process::id(), model = %args.model_path.display(), "disposable inference worker started");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut input = BufReader::new(stdin());
        loop {
            match read_frame(&mut input) {
                Ok(frame) => {
                    if tx.send(Ok(frame)).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = tx.send(Err(error));
                    break;
                }
            }
        }
    });
    let mut output = stdout();
    let mut engine: Option<ParakeetNative> = None;
    let idle_timeout = Duration::from_secs(args.idle_timeout_secs.max(1));
    loop {
        let frame = match rx.recv_timeout(idle_timeout) {
            Ok(Ok(frame)) => frame,
            Ok(Err(error)) => {
                tracing::warn!(%error, "worker input stream closed");
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                tracing::info!(
                    timeout_secs = idle_timeout.as_secs(),
                    model_loaded = engine.is_some(),
                    "worker idle timeout reached; exiting process"
                );
                break;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
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
    Ok(())
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
