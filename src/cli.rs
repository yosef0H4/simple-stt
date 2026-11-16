//! Non-interactive media commands. Service access is supplied by the host so
//! this module never loads speech models or sends text through a shell.
use crate::config::SpeechLanguage;
use anyhow::{bail, Context, Result};
use clap::Subcommand;
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_TEXT_BYTES: usize = 1024 * 1024;
const MAX_WAV_BYTES: u64 = 512 * 1024 * 1024;
const FFMPEG_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const SAMPLE_RATE: u32 = 16_000;
const TARGET_CHUNK_SECONDS: u64 = 25;
const MAX_CHUNK_SECONDS: u64 = 35;
const SEARCH_BEFORE_SECONDS: u64 = 5;
const SEARCH_AFTER_SECONDS: u64 = 5;

/// The narrow host boundary for media operations. Implementations should use
/// the existing capture and reader services rather than loading engines here.
pub trait Backend {
    fn transcribe_wav(&mut self, wav: &Path, language: Option<&str>) -> Result<String>;
    /// `output == None` requests playback through the installed reader service.
    fn speak_text(&mut self, text: &str, output: Option<&Path>) -> Result<()>;
    fn ocr_image(&mut self, image: &Path) -> Result<String>;
}

#[derive(Debug, Subcommand)]
pub enum MediaCommand {
    /// Transcribe an audio or video file.
    Transcribe {
        input: PathBuf,
        /// Override the saved speech language for this file.
        #[arg(long, value_enum)]
        language: Option<SpeechLanguage>,
        /// Emit a JSON object containing the transcript.
        #[arg(long)]
        json: bool,
    },
    /// Speak text, or save synthesized audio as a WAV file.
    Speak {
        /// Text to speak (omit when using --file or --stdin).
        text: Option<String>,
        /// Read text from a UTF-8 file.
        #[arg(long, value_name = "PATH", conflicts_with_all = ["stdin", "text"])]
        file: Option<PathBuf>,
        /// Read text from standard input.
        #[arg(long, conflicts_with = "text")]
        stdin: bool,
        /// Save audio to a WAV file instead of reader playback.
        #[arg(long, value_name = "PATH")]
        output: Option<PathBuf>,
    },
    /// Extract text from an image file.
    Ocr {
        image: PathBuf,
        /// Emit a JSON object containing the recognized text.
        #[arg(long)]
        json: bool,
    },
}

pub fn dispatch_media<B: Backend>(command: MediaCommand, backend: &mut B) -> Result<()> {
    match command {
        MediaCommand::Transcribe {
            input,
            language,
            json,
        } => {
            let language = language.map(SpeechLanguage::as_str);
            let text = transcribe_file(backend, &input, language)?;
            print_text(&text, json)
        }
        MediaCommand::Speak {
            text,
            file,
            stdin,
            output,
        } => {
            let text = match (text, file, stdin) {
                (Some(text), None, false) => text,
                (None, Some(path), false) => {
                    let file = File::open(&path)
                        .with_context(|| format!("opening text file {}", path.display()))?;
                    read_limited(file, "text file")?
                }
                (None, None, true) => read_limited(io::stdin(), "stdin")?,
                (None, None, false) if !io::stdin().is_terminal() => {
                    read_limited(io::stdin(), "stdin")?
                }
                (None, None, false) => bail!("provide text, --file, or pipe text to stdin"),
                _ => bail!("provide exactly one of positional text, --file, or --stdin"),
            };
            anyhow::ensure!(text.len() <= MAX_TEXT_BYTES, "text exceeds 1 MiB");
            anyhow::ensure!(!text.trim().is_empty(), "text is empty");
            backend.speak_text(&text, output.as_deref())
        }
        MediaCommand::Ocr { image, json } => {
            anyhow::ensure!(
                image.is_file(),
                "image file does not exist: {}",
                image.display()
            );
            let text = backend.ocr_image(&image)?;
            print_text(&text, json)
        }
    }
}

/// Transcribe an audio or video file without writing a CLI representation.
pub fn transcribe_file<B: Backend>(
    backend: &mut B,
    input: &Path,
    language: Option<&str>,
) -> Result<String> {
    anyhow::ensure!(
        input.is_file(),
        "input file does not exist: {}",
        input.display()
    );
    let wav = TempWav::create()?;
    decode_to_wav(input, &wav.path)?;
    transcribe_chunks(backend, &wav.path, language, &wav.dir)
}

fn read_limited(mut reader: impl Read, source: &str) -> Result<String> {
    let mut bytes = Vec::with_capacity(4096);
    reader
        .by_ref()
        .take((MAX_TEXT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .with_context(|| format!("reading {source}"))?;
    anyhow::ensure!(bytes.len() <= MAX_TEXT_BYTES, "text exceeds 1 MiB");
    String::from_utf8(bytes).context("text input is not valid UTF-8")
}

fn print_text(text: &str, json: bool) -> Result<()> {
    if json {
        #[derive(Serialize)]
        struct TextResult<'a> {
            text: &'a str,
        }
        println!("{}", serde_json::to_string(&TextResult { text })?);
    } else {
        println!("{text}");
    }
    Ok(())
}

fn decode_to_wav(input: &Path, output: &Path) -> Result<()> {
    let mut child = Command::new("ffmpeg")
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(input)
        .args([
            "-vn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-c:a",
            "pcm_s16le",
            "-f",
            "wav",
        ])
        .arg(output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("starting ffmpeg (install ffmpeg to transcribe media files)")?;
    let start = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if start.elapsed() > FFMPEG_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            bail!("ffmpeg exceeded the 30 minute conversion limit");
        }
        if fs::metadata(output).is_ok_and(|m| m.len() > MAX_WAV_BYTES) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("decoded audio exceeds the 512 MiB limit");
        }
        thread::sleep(Duration::from_millis(50));
    }
    let status = child.wait()?;
    anyhow::ensure!(status.success(), "ffmpeg could not decode media");
    let wav_len = fs::metadata(output)
        .context("ffmpeg produced no WAV output")?
        .len();
    anyhow::ensure!(
        wav_len > 44 && wav_len <= MAX_WAV_BYTES,
        "decoded WAV is empty or exceeds 512 MiB"
    );
    Ok(())
}

/// Feed bounded, non-overlapping PCM ranges to the existing worker. Candidate
/// boundaries are the quietest 100 ms windows near each 25 second target.
/// Every source sample belongs to exactly one range; there is no overlap to
/// silently remove repeated words from adjacent recognition results.
fn transcribe_chunks<B: Backend>(
    backend: &mut B,
    source: &Path,
    language: Option<&str>,
    temp_dir: &Path,
) -> Result<String> {
    let mut wav = File::open(source).context("opening decoded WAV")?;
    let (data_offset, data_len) = pcm_data_chunk(&mut wav)?;
    let samples = data_len / 2;
    anyhow::ensure!(samples > 0, "decoded WAV contains no PCM samples");
    let mut start = 0_u64;
    let mut index = 0_u32;
    let mut transcripts = Vec::new();
    let hard_limit = SAMPLE_RATE as u64 * MAX_CHUNK_SECONDS;
    let target = SAMPLE_RATE as u64 * TARGET_CHUNK_SECONDS;
    while start < samples {
        let remaining = samples - start;
        let end = if remaining <= hard_limit {
            samples
        } else {
            choose_boundary(&mut wav, data_offset, start, samples, target, hard_limit)?
        };
        anyhow::ensure!(
            end > start && end - start <= hard_limit,
            "invalid audio chunk boundary"
        );
        let chunk_path = temp_dir.join(format!("chunk-{index}.wav"));
        write_pcm_wav_range(&mut wav, data_offset, start, end - start, &chunk_path)?;
        let text = backend.transcribe_wav(&chunk_path, language)?;
        if !text.trim().is_empty() {
            transcripts.push(text.trim().to_owned());
        }
        fs::remove_file(&chunk_path).context("removing temporary audio chunk")?;
        start = end;
        index += 1;
    }
    Ok(transcripts.join(" "))
}

fn pcm_data_chunk(file: &mut File) -> Result<(u64, u64)> {
    let len = file.metadata()?.len();
    let mut header = [0_u8; 12];
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut header).context("reading WAV header")?;
    anyhow::ensure!(
        &header[..4] == b"RIFF" && &header[8..12] == b"WAVE",
        "ffmpeg output is not a RIFF/WAVE file"
    );
    let mut offset = 12_u64;
    let mut pcm16_mono = false;
    loop {
        anyhow::ensure!(offset + 8 <= len, "WAV is missing its PCM data chunk");
        file.seek(SeekFrom::Start(offset))?;
        let mut chunk_header = [0_u8; 8];
        file.read_exact(&mut chunk_header)?;
        let chunk_len = u32::from_le_bytes(chunk_header[4..8].try_into().unwrap()) as u64;
        let content = offset + 8;
        let next = content
            .checked_add(chunk_len)
            .context("WAV chunk length overflow")?
            + (chunk_len & 1);
        anyhow::ensure!(next <= len, "WAV chunk extends past end of file");
        match &chunk_header[..4] {
            b"fmt " => {
                anyhow::ensure!(chunk_len >= 16, "invalid WAV format chunk");
                let mut fmt = [0_u8; 16];
                file.seek(SeekFrom::Start(content))?;
                file.read_exact(&mut fmt)?;
                pcm16_mono = u16::from_le_bytes([fmt[0], fmt[1]]) == 1
                    && u16::from_le_bytes([fmt[2], fmt[3]]) == 1
                    && u32::from_le_bytes(fmt[4..8].try_into().unwrap()) == SAMPLE_RATE
                    && u16::from_le_bytes([fmt[14], fmt[15]]) == 16;
            }
            b"data" => {
                anyhow::ensure!(pcm16_mono, "WAV must contain 16 kHz mono PCM16 audio");
                anyhow::ensure!(
                    chunk_len.is_multiple_of(2),
                    "WAV PCM data has an incomplete sample"
                );
                return Ok((content, chunk_len));
            }
            _ => {}
        }
        offset = next;
    }
}

fn choose_boundary(
    file: &mut File,
    data_offset: u64,
    start: u64,
    samples: u64,
    target: u64,
    hard_limit: u64,
) -> Result<u64> {
    let nominal = start + target;
    let low = start + target.saturating_sub(SAMPLE_RATE as u64 * SEARCH_BEFORE_SECONDS);
    let high = (start + target + SAMPLE_RATE as u64 * SEARCH_AFTER_SECONDS)
        .min(start + hard_limit)
        .min(samples - 1);
    let window = SAMPLE_RATE as u64 / 10;
    let first = low / window * window;
    let mut best_start = nominal.min(high).max(low);
    let mut best_energy = u64::MAX;
    let mut cursor = first;
    let mut block = vec![0_u8; (window * 2) as usize];
    while cursor + window <= high {
        file.seek(SeekFrom::Start(data_offset + cursor * 2))?;
        file.read_exact(&mut block)?;
        let energy = block.as_chunks::<2>().0.iter().fold(0_u64, |sum, sample| {
            sum + i16::from_le_bytes(*sample).unsigned_abs() as u64
        });
        if energy < best_energy {
            best_energy = energy;
            best_start = cursor + window / 2;
        }
        cursor += window;
    }
    Ok(best_start.clamp(start + 1, (start + hard_limit).min(samples - 1)))
}

fn write_pcm_wav_range(
    file: &mut File,
    data_offset: u64,
    start: u64,
    count: u64,
    output: &Path,
) -> Result<()> {
    let data_bytes = u32::try_from(count * 2).context("audio chunk is too large for WAV")?;
    let mut target = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    target.write_all(b"RIFF")?;
    target.write_all(&(36_u32 + data_bytes).to_le_bytes())?;
    target.write_all(b"WAVEfmt ")?;
    target.write_all(&16_u32.to_le_bytes())?;
    target.write_all(&1_u16.to_le_bytes())?;
    target.write_all(&1_u16.to_le_bytes())?;
    target.write_all(&SAMPLE_RATE.to_le_bytes())?;
    target.write_all(&(SAMPLE_RATE * 2).to_le_bytes())?;
    target.write_all(&2_u16.to_le_bytes())?;
    target.write_all(&16_u16.to_le_bytes())?;
    target.write_all(b"data")?;
    target.write_all(&data_bytes.to_le_bytes())?;
    file.seek(SeekFrom::Start(data_offset + start * 2))?;
    let mut left = data_bytes as usize;
    let mut buffer = [0_u8; 64 * 1024];
    while left > 0 {
        let take = left.min(buffer.len());
        file.read_exact(&mut buffer[..take])?;
        target.write_all(&buffer[..take])?;
        left -= take;
    }
    target.flush()?;
    Ok(())
}

struct TempWav {
    dir: PathBuf,
    path: PathBuf,
}
impl TempWav {
    fn create() -> Result<Self> {
        let root = std::env::temp_dir();
        for attempt in 0..32_u32 {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let dir = root.join(format!(
                "simple-stt-media-{}-{stamp}-{attempt}",
                std::process::id()
            ));
            match fs::create_dir(&dir) {
                Ok(()) => {
                    let path = dir.join("audio.wav");
                    OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)
                        .context("creating temporary WAV")?;
                    return Ok(Self { dir, path });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e).context("creating temporary media directory"),
            }
        }
        bail!("could not allocate a unique temporary media directory")
    }
}
impl Drop for TempWav {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[derive(Default)]
    struct RecordingBackend {
        chunks: Vec<Vec<i16>>,
    }

    impl Backend for RecordingBackend {
        fn transcribe_wav(&mut self, wav: &Path, _language: Option<&str>) -> Result<String> {
            let mut file = File::open(wav)?;
            let (offset, len) = pcm_data_chunk(&mut file)?;
            file.seek(SeekFrom::Start(offset))?;
            let mut bytes = vec![0; len as usize];
            file.read_exact(&mut bytes)?;
            let samples = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            self.chunks.push(samples);
            Ok("chunk".into())
        }

        fn speak_text(&mut self, _text: &str, _output: Option<&Path>) -> Result<()> {
            Ok(())
        }

        fn ocr_image(&mut self, _image: &Path) -> Result<String> {
            Ok(String::new())
        }
    }

    fn write_test_wav(path: &Path, samples: &[i16]) {
        let data_bytes = (samples.len() * 2) as u32;
        let mut file = File::create(path).unwrap();
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36_u32 + data_bytes).to_le_bytes())
            .unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16_u32.to_le_bytes()).unwrap();
        file.write_all(&1_u16.to_le_bytes()).unwrap();
        file.write_all(&1_u16.to_le_bytes()).unwrap();
        file.write_all(&SAMPLE_RATE.to_le_bytes()).unwrap();
        file.write_all(&(SAMPLE_RATE * 2).to_le_bytes()).unwrap();
        file.write_all(&2_u16.to_le_bytes()).unwrap();
        file.write_all(&16_u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&data_bytes.to_le_bytes()).unwrap();
        for sample in samples {
            file.write_all(&sample.to_le_bytes()).unwrap();
        }
    }

    #[test]
    fn long_audio_is_partitioned_at_nearby_silence_without_losing_samples() {
        let temp = TempWav::create().unwrap();
        let source = temp.dir.join("source.wav");
        let silence_start = SAMPLE_RATE as usize * 25 - SAMPLE_RATE as usize / 10;
        let silence_end = silence_start + SAMPLE_RATE as usize / 5;
        let samples: Vec<i16> = (0..SAMPLE_RATE as usize * 61)
            .map(|index| {
                if (silence_start..silence_end).contains(&index) {
                    0
                } else {
                    900 + (index % 97) as i16
                }
            })
            .collect();
        write_test_wav(&source, &samples);

        let mut backend = RecordingBackend::default();
        let transcript = transcribe_chunks(&mut backend, &source, None, &temp.dir).unwrap();

        assert_eq!(backend.chunks.len(), 3);
        assert!(backend
            .chunks
            .iter()
            .all(|chunk| chunk.len() <= SAMPLE_RATE as usize * MAX_CHUNK_SECONDS as usize));
        let first_end = backend.chunks[0].len();
        assert!((silence_start..=silence_end).contains(&first_end));
        let reconstructed: Vec<i16> = backend.chunks.into_iter().flatten().collect();
        assert_eq!(reconstructed, samples);
        assert_eq!(transcript, "chunk chunk chunk");
    }

    #[test]
    fn short_audio_is_sent_as_one_unchanged_chunk() {
        let temp = TempWav::create().unwrap();
        let source = temp.dir.join("source.wav");
        let samples: Vec<i16> = (0..SAMPLE_RATE as usize * 3)
            .map(|sample| sample as i16)
            .collect();
        write_test_wav(&source, &samples);
        let mut backend = RecordingBackend::default();

        transcribe_chunks(&mut backend, &source, None, &temp.dir).unwrap();

        assert_eq!(backend.chunks, vec![samples]);
    }

    #[test]
    fn invalid_and_truncated_wavs_are_rejected() {
        let temp = TempWav::create().unwrap();
        let invalid = temp.dir.join("invalid.wav");
        fs::write(&invalid, b"not a wave").unwrap();
        assert!(pcm_data_chunk(&mut File::open(invalid).unwrap()).is_err());

        let truncated = temp.dir.join("truncated.wav");
        write_test_wav(&truncated, &[1, 2, 3, 4]);
        let mut bytes = fs::read(&truncated).unwrap();
        bytes[40..44].copy_from_slice(&100_u32.to_le_bytes());
        fs::write(&truncated, bytes).unwrap();
        assert!(pcm_data_chunk(&mut File::open(truncated).unwrap()).is_err());
    }

    #[test]
    fn text_reader_enforces_limit_before_accepting_input() {
        let under_limit = vec![b'a'; MAX_TEXT_BYTES];
        assert_eq!(
            read_limited(Cursor::new(under_limit), "test input")
                .unwrap()
                .len(),
            MAX_TEXT_BYTES
        );
        let over_limit = vec![b'a'; MAX_TEXT_BYTES + 1];
        assert!(read_limited(Cursor::new(over_limit), "test input").is_err());
    }
}
