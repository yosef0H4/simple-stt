use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

pub const MAX_CONTROL_FRAME: usize = 64 * 1024;
pub const MAX_PCM_FRAME: usize = 32 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerRequest {
    Hello {
        protocol: u16,
        incarnation: String,
    },
    Prepare {
        device: String,
    },
    Synthesize {
        job_id: String,
        generation: u64,
        sentence: u32,
        unit: u32,
        text: String,
        voice: String,
        #[serde(default)]
        steps: Option<u32>,
        language: String,
        speed: f32,
        device: String,
    },
    Recognize {
        job_id: String,
        generation: u64,
        image_path: String,
        language: String,
    },
    Cancel {
        job_id: String,
        generation: u64,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerEvent {
    Ready {
        incarnation: String,
        engine: String,
        device: String,
    },
    Pcm {
        job_id: String,
        generation: u64,
        sentence: u32,
        unit: u32,
        sample_rate: u32,
        sample_offset: u64,
        final_chunk: bool,
    },
    Text {
        job_id: String,
        generation: u64,
        text: String,
    },
    Finished {
        job_id: String,
        generation: u64,
    },
    Cancelled {
        job_id: String,
        generation: u64,
    },
    Error {
        job_id: Option<String>,
        generation: Option<u64>,
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PcmHeader {
    pub job_id: String,
    pub generation: u64,
    pub sentence: u32,
    pub unit: u32,
    pub sample_rate: u32,
    pub sample_offset: u64,
    pub final_chunk: bool,
    pub sample_count: u32,
}

#[derive(Debug)]
pub enum WorkerFrame<T> {
    Control(T),
    Pcm {
        header: PcmHeader,
        samples: Vec<i16>,
    },
}

pub fn write_json_frame<T: Serialize>(writer: &mut impl Write, value: &T) -> Result<()> {
    let payload = serde_json::to_vec(value).context("encoding worker control frame")?;
    let framed_len = payload.len().saturating_add(1);
    if framed_len > MAX_CONTROL_FRAME {
        bail!("worker control frame exceeds 64 KiB");
    }
    writer.write_all(&(framed_len as u32).to_le_bytes())?;
    writer.write_all(&[0])?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

pub fn read_json_frame<T: for<'de> Deserialize<'de>>(reader: &mut impl Read) -> Result<T> {
    let mut len = [0_u8; 4];
    reader
        .read_exact(&mut len)
        .context("reading worker control frame length")?;
    let len = u32::from_le_bytes(len) as usize;
    if !(2..=MAX_CONTROL_FRAME).contains(&len) {
        bail!("invalid worker frame length: {len}");
    }
    let mut kind = [0_u8; 1];
    reader.read_exact(&mut kind)?;
    if kind[0] != 0 {
        bail!("unexpected binary worker frame on control input");
    }
    let mut payload = vec![0; len - 1];
    reader
        .read_exact(&mut payload)
        .context("reading worker control frame body")?;
    serde_json::from_slice(&payload).context("decoding worker control frame")
}

pub fn write_pcm_frame(writer: &mut impl Write, header: &PcmHeader, samples: &[i16]) -> Result<()> {
    validate_pcm_packet(header.sample_rate, samples)?;
    if header.sample_count as usize != samples.len() {
        bail!("PCM header sample count mismatch")
    }
    let metadata = serde_json::to_vec(header).context("encoding PCM metadata")?;
    let len = 1usize + 4 + metadata.len() + samples.len() * 2;
    if len > MAX_CONTROL_FRAME {
        bail!("PCM frame exceeds bounded frame size")
    }
    writer.write_all(&(len as u32).to_le_bytes())?;
    writer.write_all(&[1])?;
    writer.write_all(&(metadata.len() as u32).to_le_bytes())?;
    writer.write_all(&metadata)?;
    for sample in samples {
        writer.write_all(&sample.to_le_bytes())?;
    }
    writer.flush()?;
    Ok(())
}

pub fn read_worker_frame<T: for<'de> Deserialize<'de>>(
    reader: &mut impl Read,
) -> Result<WorkerFrame<T>> {
    let mut len = [0_u8; 4];
    reader
        .read_exact(&mut len)
        .context("reading worker frame length")?;
    let len = u32::from_le_bytes(len) as usize;
    if !(2..=MAX_CONTROL_FRAME).contains(&len) {
        bail!("invalid worker frame length: {len}")
    }
    let mut kind = [0_u8; 1];
    reader.read_exact(&mut kind)?;
    match kind[0] {
        0 => {
            let mut body = vec![0; len - 1];
            reader.read_exact(&mut body)?;
            Ok(WorkerFrame::Control(
                serde_json::from_slice(&body).context("decoding worker control event")?,
            ))
        }
        1 => {
            if len < 6 {
                bail!("truncated PCM frame")
            };
            let mut mlen = [0_u8; 4];
            reader.read_exact(&mut mlen)?;
            let mlen = u32::from_le_bytes(mlen) as usize;
            if mlen == 0 || mlen > MAX_CONTROL_FRAME || mlen + 5 > len {
                bail!("invalid PCM metadata length")
            };
            let mut meta = vec![0; mlen];
            reader.read_exact(&mut meta)?;
            let header: PcmHeader =
                serde_json::from_slice(&meta).context("decoding PCM metadata")?;
            let raw_len = len - 1 - 4 - mlen;
            if !raw_len.is_multiple_of(2)
                || raw_len / 2 != header.sample_count as usize
                || raw_len > MAX_PCM_FRAME
            {
                bail!("invalid PCM payload length")
            };
            let mut raw = vec![0; raw_len];
            reader.read_exact(&mut raw)?;
            let (chunks, _) = raw.as_chunks::<2>();
            let samples: Vec<i16> = chunks
                .iter()
                .map(|bytes| i16::from_le_bytes(*bytes))
                .collect();
            validate_pcm_packet(header.sample_rate, &samples)?;
            Ok(WorkerFrame::Pcm { header, samples })
        }
        _ => bail!("unknown worker frame type"),
    }
}

pub fn validate_pcm_packet(sample_rate: u32, samples: &[i16]) -> Result<()> {
    if !(8_000..=192_000).contains(&sample_rate) {
        bail!("unsupported PCM sample rate");
    }
    if samples.len().saturating_mul(2) > MAX_PCM_FRAME {
        bail!("PCM packet exceeds 32 KiB");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    #[test]
    fn synthesis_steps_round_trip_and_old_messages_default_to_none() {
        let mut value = serde_json::json!({"type":"synthesize", "job_id":"a", "generation":1, "sentence":0, "unit":0, "text":"hello", "voice":"sid:2", "language":"en", "speed":1.0, "device":"cpu", "steps":9});
        let request: WorkerRequest = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(request).unwrap()["steps"], 9);
        value.as_object_mut().unwrap().remove("steps");
        assert!(matches!(
            serde_json::from_value::<WorkerRequest>(value).unwrap(),
            WorkerRequest::Synthesize { steps: None, .. }
        ));
    }
    #[test]
    fn framed_messages_round_trip_and_bound_allocations() {
        let request = WorkerRequest::Cancel {
            job_id: "job".into(),
            generation: 9,
        };
        let mut bytes = Vec::new();
        write_json_frame(&mut bytes, &request).unwrap();
        let decoded: WorkerRequest = read_json_frame(&mut Cursor::new(bytes)).unwrap();
        assert!(matches!(
            decoded,
            WorkerRequest::Cancel { generation: 9, .. }
        ));
        assert!(read_json_frame::<WorkerRequest>(&mut Cursor::new([255, 255, 255, 255])).is_err());
    }
    #[test]
    fn pcm_is_transmitted_as_bounded_binary_with_sample_timeline() {
        let header = PcmHeader {
            job_id: "job".into(),
            generation: 4,
            sentence: 2,
            unit: 1,
            sample_rate: 24_000,
            sample_offset: 12,
            final_chunk: true,
            sample_count: 3,
        };
        let samples = [-10, 0, 12];
        let mut bytes = Vec::new();
        write_pcm_frame(&mut bytes, &header, &samples).unwrap();
        match read_worker_frame::<WorkerEvent>(&mut Cursor::new(bytes)).unwrap() {
            WorkerFrame::Pcm {
                header: received,
                samples: audio,
            } => {
                assert_eq!(received.sample_offset, 12);
                assert_eq!(audio, samples)
            }
            _ => panic!("expected PCM"),
        }
    }
}
