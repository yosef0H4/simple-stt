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
        let mut output = Vec::new();
        while self.phase + 1.0 < source.len() as f64 {
            let left = self.phase.floor() as usize;
            let fraction = (self.phase - left as f64) as f32;
            let sample = source[left] * (1.0 - fraction) + source[left + 1] * fraction;
            output.push(sample);
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
    let clipped = value.clamp(-1.0, 1.0);
    (clipped * i16::MAX as f32).round() as i16
}

#[derive(Debug)]
pub struct FrameAssembler {
    resampler: LinearResampler,
    channels: usize,
    gain: f32,
    frame_samples: usize,
    pending: Vec<i16>,
}

impl FrameAssembler {
    pub fn new(input_rate: u32, channels: usize, gain: f32, frame_samples: usize) -> Self {
        Self {
            resampler: LinearResampler::new(input_rate, 16_000),
            channels,
            gain,
            frame_samples,
            pending: Vec::new(),
        }
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
        while self.pending.len() >= self.frame_samples {
            frames.push(self.pending.drain(..self.frame_samples).collect());
        }
        frames
    }
}

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
    fn clamps_samples() {
        assert_eq!(f32_to_i16(2.0), i16::MAX);
        assert!(f32_to_i16(-2.0) <= -32766);
    }

    #[test]
    fn resamples_48k_to_16k_approximately() {
        let mut resampler = LinearResampler::new(48_000, 16_000);
        let output = resampler.process(&vec![0.25; 4_800]);
        assert!((1_599..=1_601).contains(&output.len()));
        assert!(output.iter().all(|value| (*value - 0.25).abs() < 0.001));
    }

    #[test]
    fn assembler_emits_fixed_frames() {
        let mut assembler = FrameAssembler::new(16_000, 1, 1.0, 320);
        let frames = assembler.push(&vec![0.5; 641]);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].len(), 320);
    }
}
