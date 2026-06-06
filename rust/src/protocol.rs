use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::io::{Read, Write};

pub const FRAME_JSON: u8 = 1;
pub const FRAME_PCM16: u8 = 2;
pub const MAX_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub kind: u8,
    pub payload: Vec<u8>,
}

pub fn write_frame(mut writer: impl Write, kind: u8, payload: &[u8]) -> Result<()> {
    if payload.len() > MAX_PAYLOAD_BYTES {
        bail!("payload too large: {} bytes", payload.len());
    }
    writer.write_all(&[kind])?;
    writer.write_all(&(payload.len() as u32).to_le_bytes())?;
    writer.write_all(payload)?;
    Ok(())
}

pub fn read_frame(mut reader: impl Read) -> Result<Frame> {
    let mut header = [0u8; 5];
    reader.read_exact(&mut header)?;
    let kind = header[0];
    if !matches!(kind, FRAME_JSON | FRAME_PCM16) {
        bail!("unsupported frame kind: {kind}");
    }
    let length = u32::from_le_bytes(header[1..5].try_into().unwrap()) as usize;
    if length > MAX_PAYLOAD_BYTES {
        bail!("payload too large: {length} bytes");
    }
    let mut payload = vec![0u8; length];
    reader.read_exact(&mut payload)?;
    Ok(Frame { kind, payload })
}

pub fn write_json(mut writer: impl Write, message: &Value) -> Result<()> {
    let payload = serde_json::to_vec(message)?;
    write_frame(&mut writer, FRAME_JSON, &payload)
}

pub fn parse_json(frame: &Frame) -> Result<Value> {
    anyhow::ensure!(frame.kind == FRAME_JSON, "expected JSON frame");
    serde_json::from_slice(&frame.payload).context("parsing JSON frame")
}

pub fn write_pcm16(mut writer: impl Write, session_id: u64, samples: &[i16]) -> Result<()> {
    let mut payload = Vec::with_capacity(8 + samples.len() * 2);
    payload.extend_from_slice(&session_id.to_le_bytes());
    for sample in samples {
        payload.extend_from_slice(&sample.to_le_bytes());
    }
    write_frame(&mut writer, FRAME_PCM16, &payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Cursor;

    #[test]
    fn json_round_trip() {
        let mut bytes = Vec::new();
        write_json(&mut bytes, &json!({"type":"commit","text":"héllo"})).unwrap();
        let frame = read_frame(Cursor::new(bytes)).unwrap();
        assert_eq!(parse_json(&frame).unwrap()["text"], "héllo");
    }

    #[test]
    fn pcm_has_session_prefix() {
        let mut bytes = Vec::new();
        write_pcm16(&mut bytes, 42, &[1, -1]).unwrap();
        let frame = read_frame(Cursor::new(bytes)).unwrap();
        assert_eq!(frame.kind, FRAME_PCM16);
        assert_eq!(&frame.payload[..8], &42u64.to_le_bytes());
    }

    #[test]
    fn rejects_unknown_kind() {
        let bytes = [99u8, 0, 0, 0, 0];
        assert!(read_frame(Cursor::new(bytes)).is_err());
    }
}
