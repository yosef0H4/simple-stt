use anyhow::Result;
use crossbeam_channel::Sender;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU64},
    Arc,
};

pub const OUTPUT_SAMPLE_RATE: u32 = 16_000;
pub const OUTPUT_FRAME_SAMPLES: usize = 320; // 20 ms

#[derive(Debug, Clone)]
pub enum AudioEvent {
    StreamError { generation: u64, error: String },
    DeviceTopologyChanged,
}

static NEXT_STREAM_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDeviceInfo {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDeviceSelection {
    pub id: String,
    pub label: String,
    pub using_default_fallback: bool,
}

pub fn choose_input_device_id(
    preferred_id: &str,
    default_id: Option<&str>,
    available_ids: &[&str],
) -> Option<(String, bool)> {
    if preferred_id.is_empty() {
        return default_id.map(|id| (id.to_owned(), false));
    }
    if available_ids.contains(&preferred_id) {
        return Some((preferred_id.to_owned(), false));
    }
    default_id.map(|id| (id.to_owned(), true))
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
    visualizer_level_from_rms(raw_rms(frame))
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AudioLevelMeter;
impl AudioLevelMeter {
    pub fn new() -> Self {
        Self
    }
    pub fn reset(&mut self) {}
    pub fn update(&mut self, rms: f32) -> AudioLevelReading {
        let dbfs = rms_dbfs(rms);
        AudioLevelReading {
            rms,
            dbfs,
            level: visualizer_level_from_rms(rms),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AudioLevelReading {
    pub rms: f32,
    pub dbfs: f32,
    pub level: f32,
}

pub fn raw_rms(frame: &[i16]) -> f32 {
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
    mean_square.sqrt()
}

pub fn rms_dbfs(rms: f32) -> f32 {
    if rms <= 0.000_001 {
        -120.0
    } else {
        20.0 * rms.log10()
    }
}

pub fn visualizer_level_from_rms(rms: f32) -> f32 {
    let dbfs = rms_dbfs(rms);
    ((dbfs + 55.0) / 37.0).clamp(0.0, 1.0).powf(1.25)
}

#[cfg(any(windows, target_os = "linux"))]
mod platform {
    use super::*;
    use anyhow::{anyhow, Context};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{SampleFormat, Stream, StreamConfig};
    use std::sync::{atomic::Ordering, Mutex};

    pub struct CaptureHandle {
        _stream: Stream,
        selection: InputDeviceSelection,
        generation: u64,
    }
    impl CaptureHandle {
        pub fn selection(&self) -> &InputDeviceSelection {
            &self.selection
        }
        pub fn generation(&self) -> u64 {
            self.generation
        }
    }
    fn device_label(device: &cpal::Device) -> String {
        let Ok(description) = device.description() else {
            return "<unknown>".to_owned();
        };
        #[cfg(target_os = "linux")]
        if let Some(driver) = description.driver() {
            return format!("{} — {driver}", description.name());
        }
        description
            .extended()
            .first()
            .cloned()
            .or_else(|| {
                description
                    .driver()
                    .filter(|driver| *driver != description.name())
                    .map(|driver| format!("{} ({driver})", description.name()))
            })
            .unwrap_or_else(|| description.name().to_owned())
    }

    pub fn list_input_devices() -> Result<Vec<InputDeviceInfo>> {
        let host = cpal::default_host();
        Ok(host
            .input_devices()
            .context("enumerating input devices")?
            .map(|device| InputDeviceInfo {
                id: device.id().map(|id| id.to_string()).unwrap_or_default(),
                label: device_label(&device),
            })
            .collect())
    }
    fn select_device(device_contains: &str) -> Result<(cpal::Device, InputDeviceSelection)> {
        let host = cpal::default_host();
        let devices = host
            .input_devices()
            .context("enumerating microphones")?
            .collect::<Vec<_>>();
        let default_device = host.default_input_device();
        let default_id = default_device
            .as_ref()
            .and_then(|device| device.id().ok())
            .map(|id| id.to_string());
        let preferred = if device_contains.trim().is_empty() {
            String::new()
        } else if device_contains.parse::<cpal::DeviceId>().is_ok() {
            device_contains.to_owned()
        } else {
            let needle = device_contains.to_lowercase();
            devices
                .iter()
                .find(|device| device_label(device).to_lowercase().contains(&needle))
                .and_then(|device| device.id().ok())
                .map(|id| id.to_string())
                .unwrap_or_else(|| device_contains.to_owned())
        };
        let ids = devices
            .iter()
            .filter_map(|device| device.id().ok().map(|id| id.to_string()))
            .collect::<Vec<_>>();
        let available = ids.iter().map(String::as_str).collect::<Vec<_>>();
        let (selected_id, using_default_fallback) =
            choose_input_device_id(&preferred, default_id.as_deref(), &available)
                .ok_or_else(|| anyhow!("no microphone found"))?;
        let device = devices
            .into_iter()
            .find(|device| device.id().is_ok_and(|id| id.to_string() == selected_id))
            .or_else(|| {
                default_device
                    .filter(|device| device.id().is_ok_and(|id| id.to_string() == selected_id))
            })
            .ok_or_else(|| anyhow!("resolved microphone disappeared during enumeration"))?;
        let label = device_label(&device);
        Ok((
            device,
            InputDeviceSelection {
                id: selected_id,
                label,
                using_default_fallback,
            },
        ))
    }
    pub fn resolve_input_device(device_contains: &str) -> Result<InputDeviceSelection> {
        select_device(device_contains).map(|(_, selection)| selection)
    }
    pub fn start_capture(
        device_contains: &str,
        gain: f32,
        tx: Sender<Vec<i16>>,
        latest_level: Option<Arc<AtomicU32>>,
        recording_active: Option<Arc<AtomicBool>>,
        event_tx: Option<Sender<AudioEvent>>,
    ) -> Result<CaptureHandle> {
        let (device, selection) = select_device(device_contains)?;
        let preferred_id = selection.id.clone();
        let first_attempt = start_capture_device(
            device,
            selection,
            gain,
            tx.clone(),
            latest_level.clone(),
            recording_active.clone(),
            event_tx.clone(),
        );
        if first_attempt.is_ok() || device_contains.trim().is_empty() {
            return first_attempt;
        }

        let (default_device, mut fallback) = select_device("")?;
        if fallback.id == preferred_id {
            return first_attempt;
        }
        fallback.using_default_fallback = true;
        start_capture_device(
            default_device,
            fallback,
            gain,
            tx,
            latest_level,
            recording_active,
            event_tx,
        )
        .with_context(|| "opening system default after preferred microphone failed")
    }

    fn start_capture_device(
        device: cpal::Device,
        selection: InputDeviceSelection,
        gain: f32,
        tx: Sender<Vec<i16>>,
        latest_level: Option<Arc<AtomicU32>>,
        recording_active: Option<Arc<AtomicBool>>,
        event_tx: Option<Sender<AudioEvent>>,
    ) -> Result<CaptureHandle> {
        let generation = NEXT_STREAM_GENERATION.fetch_add(1, Ordering::Relaxed);
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
        let stream_context = InputStreamContext {
            assembler,
            tx,
            latest_level,
            recording_active,
            event_tx,
            generation,
        };
        let stream = match sample_format {
            SampleFormat::F32 => {
                build_stream::<f32>(&device, &config, stream_context.clone(), |v| v)?
            }
            SampleFormat::I16 => {
                build_stream::<i16>(&device, &config, stream_context.clone(), |v| {
                    v as f32 / 32768.0
                })?
            }
            SampleFormat::U16 => build_stream::<u16>(&device, &config, stream_context, |v| {
                (v as f32 - 32768.0) / 32768.0
            })?,
            other => return Err(anyhow!("unsupported microphone sample format: {other:?}")),
        };
        stream.play().context("starting microphone stream")?;
        Ok(CaptureHandle {
            _stream: stream,
            selection,
            generation,
        })
    }

    #[derive(Clone)]
    struct InputStreamContext {
        assembler: Arc<Mutex<FrameAssembler>>,
        tx: Sender<Vec<i16>>,
        latest_level: Option<Arc<AtomicU32>>,
        recording_active: Option<Arc<AtomicBool>>,
        event_tx: Option<Sender<AudioEvent>>,
        generation: u64,
    }

    fn build_stream<T>(
        device: &cpal::Device,
        config: &StreamConfig,
        context: InputStreamContext,
        convert: impl Fn(T) -> f32 + Send + Sync + Copy + 'static,
    ) -> Result<Stream>
    where
        T: cpal::SizedSample + Copy,
    {
        let InputStreamContext {
            assembler,
            tx,
            latest_level,
            recording_active,
            event_tx,
            generation,
        } = context;
        let mut converted = Vec::new();
        let mut was_recording = false;
        let mut level_meter = AudioLevelMeter::new();
        let debug_levels = std::env::var_os("SIMPLE_STT_AUDIO_LEVEL_DEBUG").is_some();
        let mut debug_frame_index: u64 = 0;
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
                    level_meter.reset();
                    was_recording = true;
                }
                converted.clear();
                converted.extend(data.iter().copied().map(convert));
                for frame in assembler.push(&converted) {
                    let raw_rms = raw_rms(&frame);
                    let reading = level_meter.update(raw_rms);
                    if let Some(level) = &latest_level {
                        level.store(reading.level.to_bits(), Ordering::Relaxed);
                    }
                    if debug_levels {
                        debug_frame_index = debug_frame_index.wrapping_add(1);
                        if debug_frame_index.is_multiple_of(5) {
                            eprintln!(
                                "audio_level raw_rms={:.6} dbfs={:.1} mapped={:.3} bars={}",
                                reading.rms,
                                reading.dbfs,
                                reading.level,
                                crate::capture::overlay::ascii_visualizer(&{
                                    let mut levels =
                                        crate::capture::overlay::empty_visualizer_levels();
                                    crate::capture::overlay::set_visualizer_level(
                                        &mut levels,
                                        reading.level,
                                    );
                                    levels
                                })
                            );
                        }
                    }
                    let _ = tx.try_send(frame);
                }
            },
            move |error| {
                tracing::error!(%error, "microphone stream error");
                if let Some(events) = &event_tx {
                    let _ = events.try_send(AudioEvent::StreamError {
                        generation,
                        error: error.to_string(),
                    });
                }
            },
            None,
        )?)
    }
}

#[cfg(windows)]
mod device_notifications {
    use super::AudioEvent;
    use anyhow::{anyhow, Result};
    use crossbeam_channel::{bounded, Sender};
    use std::thread::{self, JoinHandle};
    use windows::core::{implement, PCWSTR};
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::Media::Audio::{
        eCapture, EDataFlow, ERole, IMMDeviceEnumerator, IMMNotificationClient,
        IMMNotificationClient_Impl, MMDeviceEnumerator, DEVICE_STATE,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    #[implement(IMMNotificationClient)]
    struct NotificationClient {
        events: Sender<AudioEvent>,
    }

    impl NotificationClient {
        fn changed(&self) {
            let _ = self.events.try_send(AudioEvent::DeviceTopologyChanged);
        }
    }

    impl IMMNotificationClient_Impl for NotificationClient_Impl {
        fn OnDeviceStateChanged(&self, _: &PCWSTR, _: DEVICE_STATE) -> windows::core::Result<()> {
            self.changed();
            Ok(())
        }

        fn OnDeviceAdded(&self, _: &PCWSTR) -> windows::core::Result<()> {
            self.changed();
            Ok(())
        }

        fn OnDeviceRemoved(&self, _: &PCWSTR) -> windows::core::Result<()> {
            self.changed();
            Ok(())
        }

        fn OnDefaultDeviceChanged(
            &self,
            flow: EDataFlow,
            _: ERole,
            _: &PCWSTR,
        ) -> windows::core::Result<()> {
            if flow == eCapture {
                self.changed();
            }
            Ok(())
        }

        fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> windows::core::Result<()> {
            Ok(())
        }
    }

    pub struct DeviceNotificationGuard {
        stop: Option<std::sync::mpsc::Sender<()>>,
        thread: Option<JoinHandle<()>>,
    }

    impl Drop for DeviceNotificationGuard {
        fn drop(&mut self) {
            if let Some(stop) = self.stop.take() {
                let _ = stop.send(());
            }
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    pub fn watch_device_changes(events: Sender<AudioEvent>) -> Result<DeviceNotificationGuard> {
        let (ready_tx, ready_rx) = bounded::<Result<(), String>>(1);
        let (stop_tx, stop_rx) = std::sync::mpsc::channel();
        let thread = thread::Builder::new()
            .name("audio-device-notifications".to_owned())
            .spawn(move || run_notification_thread(events, stop_rx, ready_tx))?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(DeviceNotificationGuard {
                stop: Some(stop_tx),
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(anyhow!(error))
            }
            Err(_) => {
                let _ = thread.join();
                Err(anyhow!(
                    "audio device notification thread stopped during startup"
                ))
            }
        }
    }

    fn run_notification_thread(
        events: Sender<AudioEvent>,
        stop: std::sync::mpsc::Receiver<()>,
        ready: Sender<Result<(), String>>,
    ) {
        // The notification watcher owns a dedicated MTA so callback lifetime and COM cleanup
        // cannot race CPAL's stream threads.
        if let Err(error) = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok() } {
            let _ = ready.send(Err(error.to_string()));
            return;
        }
        let result = (|| -> windows::core::Result<()> {
            let enumerator: IMMDeviceEnumerator =
                unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
            let client: IMMNotificationClient = NotificationClient { events }.into();
            unsafe { enumerator.RegisterEndpointNotificationCallback(&client)? };
            let _ = ready.send(Ok(()));
            let _ = stop.recv();
            unsafe { enumerator.UnregisterEndpointNotificationCallback(&client)? };
            Ok(())
        })();
        unsafe { CoUninitialize() };
        if let Err(error) = result {
            let _ = ready.try_send(Err(error.to_string()));
        }
    }
}

#[cfg(windows)]
pub use device_notifications::{watch_device_changes, DeviceNotificationGuard};

#[cfg(target_os = "linux")]
pub struct DeviceNotificationGuard {
    stop: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "linux")]
impl Drop for DeviceNotificationGuard {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(target_os = "linux")]
pub fn watch_device_changes(events: Sender<AudioEvent>) -> Result<DeviceNotificationGuard> {
    let (stop_tx, stop_rx) = std::sync::mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("linux-audio-device-notifications".to_owned())
        .spawn(move || {
            let mut previous = linux_input_topology_signature();
            loop {
                match stop_rx.recv_timeout(std::time::Duration::from_millis(750)) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                }
                let current = linux_input_topology_signature();
                if current != previous {
                    previous = current;
                    let _ = events.send(AudioEvent::DeviceTopologyChanged);
                }
            }
        })?;
    Ok(DeviceNotificationGuard {
        stop: Some(stop_tx),
        thread: Some(thread),
    })
}

#[cfg(target_os = "linux")]
fn linux_input_topology_signature() -> (Vec<String>, Option<String>) {
    let mut ids = list_input_devices()
        .unwrap_or_default()
        .into_iter()
        .map(|device| device.id)
        .collect::<Vec<_>>();
    ids.sort();
    let default = resolve_input_device("").ok().map(|device| device.id);
    (ids, default)
}

#[cfg(not(any(windows, target_os = "linux")))]
pub struct DeviceNotificationGuard;

#[cfg(not(any(windows, target_os = "linux")))]
pub fn watch_device_changes(_: Sender<AudioEvent>) -> Result<DeviceNotificationGuard> {
    Ok(DeviceNotificationGuard)
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use super::*;
    use anyhow::bail;
    pub struct CaptureHandle;
    impl CaptureHandle {
        pub fn selection(&self) -> &InputDeviceSelection {
            unreachable!()
        }
        pub fn generation(&self) -> u64 {
            unreachable!()
        }
    }
    pub fn list_input_devices() -> Result<Vec<InputDeviceInfo>> {
        bail!("microphone capture is not implemented for this platform")
    }
    pub fn resolve_input_device(_: &str) -> Result<InputDeviceSelection> {
        bail!("microphone capture is not implemented for this platform")
    }
    pub fn start_capture(
        _: &str,
        _: f32,
        _: Sender<Vec<i16>>,
        _: Option<Arc<AtomicU32>>,
        _: Option<Arc<AtomicBool>>,
        _: Option<Sender<AudioEvent>>,
    ) -> Result<CaptureHandle> {
        bail!("microphone capture is not implemented for this platform")
    }
}
pub use platform::{list_input_devices, resolve_input_device, start_capture, CaptureHandle};

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

    #[test]
    fn visualizer_level_uses_dbfs_noise_floor() {
        assert_eq!(visualizer_level_from_rms(0.0001), 0.0);
        let speech = visualizer_level_from_rms(0.03);
        assert!(speech > 0.4 && speech < 1.0, "{speech}");
        assert_eq!(visualizer_level_from_rms(1.0), 1.0);
    }

    #[test]
    fn loudness_meter_tracks_absolute_level() {
        let mut meter = AudioLevelMeter::new();
        let quiet = meter.update(0.001);
        let speech = meter.update(0.16);
        assert!(
            quiet.level < speech.level,
            "louder audio should produce a higher level: quiet={quiet:?} speech={speech:?}"
        );
        assert!(
            (speech.level - visualizer_level_from_rms(0.16)).abs() < f32::EPSILON,
            "meter should use direct loudness mapping: {speech:?}"
        );
    }

    #[test]
    fn preferred_microphone_falls_back_and_returns() {
        let available = ["default", "preferred"];
        assert_eq!(
            choose_input_device_id("preferred", Some("default"), &available),
            Some(("preferred".to_owned(), false))
        );
        assert_eq!(
            choose_input_device_id("preferred", Some("default"), &["default"]),
            Some(("default".to_owned(), true))
        );
        assert_eq!(
            choose_input_device_id("preferred", Some("other"), &["other", "preferred"]),
            Some(("preferred".to_owned(), false))
        );
    }

    #[test]
    fn automatic_mode_tracks_the_default() {
        assert_eq!(
            choose_input_device_id("", Some("first"), &["first", "second"]),
            Some(("first".to_owned(), false))
        );
        assert_eq!(
            choose_input_device_id("", Some("second"), &["first", "second"]),
            Some(("second".to_owned(), false))
        );
    }
}
