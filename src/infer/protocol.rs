use anyhow::{bail, Context, Result};
use std::io::{Read, Write};

pub const MAGIC: [u8; 4] = *b"UVX1";
pub const VERSION: u16 = 1;
pub const MAX_BODY_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum MessageType {
    Hello = 1,
    HelloAck = 2,
    TranscribePcm = 3,
    TranscribeWav = 4,
    Transcript = 5,
    Error = 6,
    Shutdown = 7,
    ShutdownAck = 8,
    Ping = 9,
    Pong = 10,
    WarmUp = 11,
    ModelLoaded = 12,
    WarmUpAck = 13,
}

impl TryFrom<u16> for MessageType {
    type Error = anyhow::Error;
    fn try_from(value: u16) -> Result<Self> {
        match value {
            1 => Ok(Self::Hello),
            2 => Ok(Self::HelloAck),
            3 => Ok(Self::TranscribePcm),
            4 => Ok(Self::TranscribeWav),
            5 => Ok(Self::Transcript),
            6 => Ok(Self::Error),
            7 => Ok(Self::Shutdown),
            8 => Ok(Self::ShutdownAck),
            9 => Ok(Self::Ping),
            10 => Ok(Self::Pong),
            11 => Ok(Self::WarmUp),
            12 => Ok(Self::ModelLoaded),
            13 => Ok(Self::WarmUpAck),
            _ => bail!("unknown inference message type {value}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub kind: MessageType,
    pub session_id: u64,
    pub body: Vec<u8>,
}

impl Frame {
    pub fn empty(kind: MessageType) -> Self {
        Self {
            kind,
            session_id: 0,
            body: Vec::new(),
        }
    }
    pub fn text(kind: MessageType, session_id: u64, text: impl Into<String>) -> Self {
        Self {
            kind,
            session_id,
            body: text.into().into_bytes(),
        }
    }
    pub fn body_as_text(&self) -> Result<String> {
        String::from_utf8(self.body.clone()).context("worker frame body was not UTF-8")
    }
    pub fn transcribe_pcm(session_id: u64, sample_rate: u32, samples: &[i16]) -> Self {
        let mut body = Vec::with_capacity(8 + samples.len() * 2);
        body.extend_from_slice(&sample_rate.to_le_bytes());
        body.extend_from_slice(&(samples.len() as u32).to_le_bytes());
        for sample in samples {
            body.extend_from_slice(&sample.to_le_bytes());
        }
        Self {
            kind: MessageType::TranscribePcm,
            session_id,
            body,
        }
    }
    pub fn decode_pcm(&self) -> Result<(u32, Vec<i16>)> {
        anyhow::ensure!(self.kind == MessageType::TranscribePcm, "frame is not PCM");
        anyhow::ensure!(self.body.len() >= 8, "PCM body too short");
        let sample_rate = u32::from_le_bytes(self.body[0..4].try_into().unwrap());
        let count = u32::from_le_bytes(self.body[4..8].try_into().unwrap()) as usize;
        anyhow::ensure!(
            self.body.len() == 8 + count * 2,
            "PCM sample count does not match body length"
        );
        let mut samples = Vec::with_capacity(count);
        for bytes in self.body[8..].chunks_exact(2) {
            samples.push(i16::from_le_bytes(bytes.try_into().unwrap()));
        }
        Ok((sample_rate, samples))
    }
}

pub fn write_frame(mut writer: impl Write, frame: &Frame) -> Result<()> {
    anyhow::ensure!(
        frame.body.len() <= MAX_BODY_BYTES,
        "worker frame body too large"
    );
    writer.write_all(&MAGIC)?;
    writer.write_all(&VERSION.to_le_bytes())?;
    writer.write_all(&(frame.kind as u16).to_le_bytes())?;
    writer.write_all(&(frame.body.len() as u32).to_le_bytes())?;
    writer.write_all(&frame.session_id.to_le_bytes())?;
    writer.write_all(&frame.body)?;
    writer.flush()?;
    Ok(())
}

pub fn read_frame(mut reader: impl Read) -> Result<Frame> {
    let mut header = [0_u8; 20];
    reader
        .read_exact(&mut header)
        .context("reading worker frame header")?;
    anyhow::ensure!(header[0..4] == MAGIC, "bad worker frame magic");
    let version = u16::from_le_bytes(header[4..6].try_into().unwrap());
    anyhow::ensure!(
        version == VERSION,
        "unsupported worker protocol version {version}"
    );
    let kind = MessageType::try_from(u16::from_le_bytes(header[6..8].try_into().unwrap()))?;
    let body_len = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
    anyhow::ensure!(
        body_len <= MAX_BODY_BYTES,
        "worker frame body too large: {body_len}"
    );
    let session_id = u64::from_le_bytes(header[12..20].try_into().unwrap());
    let mut body = vec![0_u8; body_len];
    reader
        .read_exact(&mut body)
        .context("reading worker frame body")?;
    Ok(Frame {
        kind,
        session_id,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn unicode_transcript_round_trip() {
        let frame = Frame::text(MessageType::Transcript, 8, "hello مرحبا 世界 🙂");
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &frame).unwrap();
        assert_eq!(read_frame(Cursor::new(bytes)).unwrap(), frame);
    }

    #[test]
    fn pcm_round_trip() {
        let frame = Frame::transcribe_pcm(9, 16_000, &[-1, 0, 2, i16::MAX]);
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &frame).unwrap();
        let decoded = read_frame(Cursor::new(bytes)).unwrap();
        assert_eq!(
            decoded.decode_pcm().unwrap(),
            (16_000, vec![-1, 0, 2, i16::MAX])
        );
    }

    #[test]
    fn version_mismatch_is_rejected() {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &Frame::empty(MessageType::Ping)).unwrap();
        bytes[4] = 99;
        assert!(read_frame(Cursor::new(bytes)).is_err());
    }

    #[test]
    fn malformed_pcm_count_is_rejected() {
        let mut frame = Frame::transcribe_pcm(1, 16_000, &[1, 2, 3]);
        frame.body[4..8].copy_from_slice(&99_u32.to_le_bytes());
        assert!(frame.decode_pcm().is_err());
    }
}
