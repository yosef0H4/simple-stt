use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use crossbeam_channel::Sender;
use std::sync::{Arc, Mutex};

use crate::resample::FrameAssembler;

pub const OUTPUT_SAMPLE_RATE: u32 = 16_000;
pub const OUTPUT_FRAME_SAMPLES: usize = 320; // 20 ms

pub struct CaptureHandle {
    _stream: Stream,
}

#[allow(deprecated)]
pub fn list_input_devices() -> Result<Vec<String>> {
    let host = cpal::default_host();
    let mut values = Vec::new();
    for device in host.input_devices().context("enumerating input devices")? {
        values.push(device.name().unwrap_or_else(|_| "<unknown>".to_owned()));
    }
    Ok(values)
}

#[allow(deprecated)]
fn select_device(device_contains: &str) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if device_contains.trim().is_empty() {
        return host
            .default_input_device()
            .ok_or_else(|| anyhow!("no default microphone found"));
    }
    let needle = device_contains.to_lowercase();
    host.input_devices()
        .context("enumerating microphones")?
        .find(|device| {
            device
                .name()
                .map(|name| name.to_lowercase().contains(&needle))
                .unwrap_or(false)
        })
        .ok_or_else(|| anyhow!("no microphone name contains {device_contains:?}"))
}

pub fn start_capture(
    device_contains: &str,
    gain: f32,
    tx: Sender<Vec<i16>>,
) -> Result<CaptureHandle> {
    let device = select_device(device_contains)?;
    let supported = device
        .default_input_config()
        .context("reading default microphone config")?;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let channels = config.channels as usize;
    let assembler = Arc::new(Mutex::new(FrameAssembler::new(
        config.sample_rate,
        channels,
        gain,
        OUTPUT_FRAME_SAMPLES,
    )));
    let error_callback = |error| tracing::error!(%error, "microphone stream error");

    let stream = match sample_format {
        SampleFormat::F32 => {
            build_stream::<f32>(&device, &config, assembler, tx, error_callback, |value| {
                value
            })?
        }
        SampleFormat::I16 => {
            build_stream::<i16>(&device, &config, assembler, tx, error_callback, |value| {
                value as f32 / 32768.0
            })?
        }
        SampleFormat::U16 => {
            build_stream::<u16>(&device, &config, assembler, tx, error_callback, |value| {
                (value as f32 - 32768.0) / 32768.0
            })?
        }
        other => return Err(anyhow!("unsupported microphone sample format: {other:?}")),
    };
    stream.play().context("starting microphone stream")?;
    Ok(CaptureHandle { _stream: stream })
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    assembler: Arc<Mutex<FrameAssembler>>,
    tx: Sender<Vec<i16>>,
    error_callback: impl FnMut(cpal::StreamError) + Send + 'static,
    convert: impl Fn(T) -> f32 + Send + Sync + Copy + 'static,
) -> Result<Stream>
where
    T: cpal::SizedSample + Copy,
{
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _| {
            let converted: Vec<f32> = data.iter().copied().map(convert).collect();
            for frame in assembler.lock().unwrap().push(&converted) {
                let _ = tx.try_send(frame);
            }
        },
        error_callback,
        None,
    )?;
    Ok(stream)
}
