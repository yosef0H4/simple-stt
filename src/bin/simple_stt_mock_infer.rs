//! Test-only disposable inference worker used by Rust integration tests.
//!
//! Release packaging explicitly builds only the three product binaries, so this
//! deterministic mock never ships in the staged application.
use anyhow::Result;
use clap::Parser;
use std::io::{stdin, stdout, BufReader};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use uvox::infer::protocol::{read_frame, write_frame, Frame, MessageType};

#[derive(Debug, Parser)]
struct Args {
    #[arg(long)]
    runtime_dir: PathBuf,
    #[arg(long)]
    model_path: PathBuf,
    #[arg(long)]
    log_path: PathBuf,
    #[arg(long, default_value = "normal")]
    log_level: String,
    #[arg(long, default_value_t = 180)]
    idle_timeout_secs: u64,
}

fn sleep_forever() -> ! {
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mode = args
        .model_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_owned();
    let _ = (
        &args.runtime_dir,
        &args.log_path,
        &args.log_level,
        args.idle_timeout_secs,
    );
    let mut input = BufReader::new(stdin());
    let mut output = stdout();
    loop {
        let frame = read_frame(&mut input)?;
        match frame.kind {
            MessageType::Hello if mode.contains("hang-handshake") => sleep_forever(),
            MessageType::Hello => write_frame(&mut output, &Frame::empty(MessageType::HelloAck))?,
            MessageType::Ping => write_frame(&mut output, &Frame::empty(MessageType::Pong))?,
            MessageType::Shutdown => {
                write_frame(&mut output, &Frame::empty(MessageType::ShutdownAck))?;
                break;
            }
            MessageType::TranscribePcm | MessageType::TranscribeWav if mode.contains("crash") => {
                std::process::exit(23)
            }
            MessageType::TranscribePcm | MessageType::TranscribeWav if mode.contains("hang") => {
                sleep_forever()
            }
            MessageType::TranscribePcm | MessageType::TranscribeWav => {
                write_frame(
                    &mut output,
                    &Frame::text(
                        MessageType::Transcript,
                        frame.session_id,
                        "mock مرحبا 世界 🙂",
                    ),
                )?;
            }
            other => write_frame(
                &mut output,
                &Frame::text(
                    MessageType::Error,
                    frame.session_id,
                    format!("mock worker rejected {other:?}"),
                ),
            )?,
        }
    }
    Ok(())
}
