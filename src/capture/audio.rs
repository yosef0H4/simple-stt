use anyhow::Result;
use crossbeam_channel::Sender;
use std::sync::{
    atomic::{AtomicBool, AtomicU32},
    Arc,
};

pub const OUTPUT_SAMPLE_RATE: u32 = 16_000;
pub const OUTPUT_FRAME_SAMPLES: usize = 320; // 20 ms

#[derive(Debug, Clone)]
pub enum AudioEvent {
    StreamError(String),
}

#[derive(Debug, Clone)]
pub struct LinearResampler {
    input_rate: f64,
    output_rate: f64,
    phase: f64,
    previous: Option<f32>,
}
impl LinearResampler {
    pub fn new(input_rate: u32, output_rate: u32) -> Self {
        assert!(input_rate > 0 && output_rate > 0);
        Self {
            input_rate: input_rate as f64,
            output_rate: output_rate as f64,
            phase: 0.0,
            previous: None,
        }
    }
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() {
            return Vec::new();
        }
        let mut source = Vec::with_capacity(input.len() + usize::from(self.previous.is_some()));
        if let Some(value) = self.previous {
            source.push(value);
        }
        source.extend_from_slice(input);
        if source.len() < 2 {
            self.previous = source.last().copied();
            return Vec::new();
        }
        let step = self.input_rate / self.output_rate;
        let mut output = Vec::with_capacity(((source.len() as f64) / step).ceil() as usize);
        while self.phase + 1.0 < source.len() as f64 {
            let left = self.phase.floor() as usize;
            let fraction = (self.phase - left as f64) as f32;
            output.push(source[left] * (1.0 - fraction) + source[left + 1] * fraction);
            self.phase += step;
        }
        self.phase -= (source.len() - 1) as f64;
        self.previous = source.last().copied();
        output
    }
}

pub fn downmix_interleaved(samples: &[f32], channels: usize) -> Vec<f32> {
    assert!(channels > 0);
    if channels == 1 {
        return samples.to_vec();
    }
    samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32)
        .collect()
}
pub fn f32_to_i16(value: f32) -> i16 {
    (value.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
}

#[derive(Debug)]
pub struct FrameAssembler {
    resampler: LinearResampler,
    channels: usize,
    gain: f32,
    frame_samples: usize,
    pending: Vec<i16>,
    pending_offset: usize,
}
impl FrameAssembler {
    pub fn new(input_rate: u32, channels: usize, gain: f32, frame_samples: usize) -> Self {
        Self {
            resampler: LinearResampler::new(input_rate, OUTPUT_SAMPLE_RATE),
            channels,
            gain,
            frame_samples,
            pending: Vec::new(),
            pending_offset: 0,
        }
    }
    pub fn reset(&mut self) {
        self.resampler = LinearResampler::new(self.resampler.input_rate as u32, OUTPUT_SAMPLE_RATE);
        self.pending.clear();
        self.pending_offset = 0;
    }
    pub fn push(&mut self, interleaved: &[f32]) -> Vec<Vec<i16>> {
        let mono = downmix_interleaved(interleaved, self.channels);
        let resampled = self.resampler.process(&mono);
        self.pending.extend(
            resampled
                .into_iter()
                .map(|value| f32_to_i16(value * self.gain)),
        );
        let mut frames = Vec::new();
        while self.pending.len() - self.pending_offset >= self.frame_samples {
            let end = self.pending_offset + self.frame_samples;
            frames.push(self.pending[self.pending_offset..end].to_vec());
            self.pending_offset = end;
        }
        if self.pending_offset == self.pending.len() {
            self.pending.clear();
            self.pending_offset = 0;
        } else if self.pending_offset >= self.frame_samples * 8 {
            self.pending.drain(..self.pending_offset);
            self.pending_offset = 0;
        }
        frames
    }
}

pub fn rms_level(frame: &[i16]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    let mean_square = frame
        .iter()
        .map(|sample| {
            let value = *sample as f32 / 32768.0;
            value * value
        })
        .sum::<f32>()
        / frame.len() as f32;
    (mean_square.sqrt() * 22.0).powf(0.62).clamp(0.0, 1.0)
}

#[cfg(windows)]
mod platform {
    use super::*;
    use anyhow::{anyhow, Context};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{SampleFormat, Stream, StreamConfig};
    use std::sync::{atomic::Ordering, Mutex};

    pub struct CaptureHandle {
        _stream: Stream,
    }
    #[allow(deprecated)]
    pub fn list_input_devices() -> Result<Vec<String>> {
        let host = cpal::default_host();
        Ok(host
            .input_devices()
            .context("enumerating input devices")?
            .map(|device| device.name().unwrap_or_else(|_| "<unknown>".to_owned()))
            .collect())
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
        latest_level: Option<Arc<AtomicU32>>,
        recording_active: Option<Arc<AtomicBool>>,
        event_tx: Option<Sender<AudioEvent>>,
    ) -> Result<CaptureHandle> {
        let device = select_device(device_contains)?;
        let supported = device
            .default_input_config()
            .context("reading default microphone config")?;
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let assembler = Arc::new(Mutex::new(FrameAssembler::new(
            config.sample_rate,
            config.channels as usize,
            gain,
            OUTPUT_FRAME_SAMPLES,
        )));
        let stream = match sample_format {
            SampleFormat::F32 => build_stream::<f32>(
                &device,
                &config,
                Arc::clone(&assembler),
                tx.clone(),
                latest_level.clone(),
                recording_active.clone(),
                event_tx.clone(),
                |v| v,
            )?,
            SampleFormat::I16 => build_stream::<i16>(
                &device,
                &config,
                Arc::clone(&assembler),
                tx.clone(),
                latest_level.clone(),
                recording_active.clone(),
                event_tx.clone(),
                |v| v as f32 / 32768.0,
            )?,
            SampleFormat::U16 => build_stream::<u16>(
                &device,
                &config,
                assembler,
                tx,
                latest_level,
                recording_active,
                event_tx,
                |v| (v as f32 - 32768.0) / 32768.0,
            )?,
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
        latest_level: Option<Arc<AtomicU32>>,
        recording_active: Option<Arc<AtomicBool>>,
        event_tx: Option<Sender<AudioEvent>>,
        convert: impl Fn(T) -> f32 + Send + Sync + Copy + 'static,
    ) -> Result<Stream>
    where
        T: cpal::SizedSample + Copy,
    {
        let mut converted = Vec::new();
        let mut was_recording = false;
        Ok(device.build_input_stream(
            config,
            move |data: &[T], _| {
                let is_recording = recording_active
                    .as_ref()
                    .map(|active| active.load(Ordering::Relaxed))
                    .unwrap_or(true);
                if !is_recording {
                    was_recording = false;
                    return;
                }
                let mut assembler = assembler.lock().unwrap();
                if !was_recording {
                    assembler.reset();
                    was_recording = true;
                }
                converted.clear();
                converted.extend(data.iter().copied().map(convert));
                for frame in assembler.push(&converted) {
                    if let Some(level) = &latest_level {
                        level.store(rms_level(&frame).to_bits(), Ordering::Relaxed);
                    }
                    let _ = tx.try_send(frame);
                }
            },
            move |error| {
                tracing::error!(%error, "microphone stream error");
                if let Some(events) = &event_tx {
                    let _ = events.try_send(AudioEvent::StreamError(error.to_string()));
                }
            },
            None,
        )?)
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;
    use anyhow::bail;
    pub struct CaptureHandle;
    pub fn list_input_devices() -> Result<Vec<String>> {
        bail!("microphone capture is Windows-only")
    }
    pub fn start_capture(
        _: &str,
        _: f32,
        _: Sender<Vec<i16>>,
        _: Option<Arc<AtomicU32>>,
        _: Option<Arc<AtomicBool>>,
        _: Option<Sender<AudioEvent>>,
    ) -> Result<CaptureHandle> {
        bail!("microphone capture is Windows-only")
    }
}
pub use platform::{list_input_devices, start_capture, CaptureHandle};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn downmixes_stereo() {
        assert_eq!(
            downmix_interleaved(&[1.0, -1.0, 0.5, 0.5], 2),
            vec![0.0, 0.5]
        );
    }
    #[test]
    fn resamples_48k_to_16k_approximately() {
        let mut r = LinearResampler::new(48_000, 16_000);
        let out = r.process(&vec![0.25; 4_800]);
        assert!((1_599..=1_601).contains(&out.len()));
    }
    #[test]
    fn assembler_emits_fixed_frames() {
        let mut a = FrameAssembler::new(16_000, 1, 1.0, 320);
        let frames = a.push(&vec![0.5; 641]);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].len(), 320);
    }
    #[test]
    fn empty_rms_is_zero() {
        assert_eq!(rms_level(&[]), 0.0);
    }
}
