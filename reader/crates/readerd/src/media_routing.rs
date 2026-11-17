//! Shared file and HTTP speech routing and fixed-rate PCM assembly.
use anyhow::Result;
use reader_core::{
    routing::{Feature, SelectionMode},
    AppConfig,
};

pub const OUTPUT_RATE: u32 = 24_000;
pub const MAX_SAMPLES: usize = OUTPUT_RATE as usize * 300;

pub fn unit_route(current: &AppConfig, text: &str) -> Result<AppConfig> {
    if current.speech.selection.mode == SelectionMode::AutoDetect {
        if let Some(language) = reader_core::language::detect(text) {
            return current.routed(Feature::Tts, Some(&language));
        }
    }
    Ok(current.clone())
}

pub fn append_pcm(output: &mut Vec<i16>, input: &[i16], rate: u32, limit: usize) -> Result<()> {
    anyhow::ensure!(rate > 0, "speech worker returned an invalid sample rate");
    let count = (input.len() as u64).saturating_mul(OUTPUT_RATE as u64) / u64::from(rate);
    let count =
        usize::try_from(count).map_err(|_| anyhow::anyhow!("audio exceeds output limit"))?;
    anyhow::ensure!(
        output.len().saturating_add(count) <= limit.min(MAX_SAMPLES),
        "audio exceeds output limit"
    );
    if rate == OUTPUT_RATE {
        output.extend_from_slice(input);
    } else if !input.is_empty() {
        for i in 0..count {
            let pos = i as f64 * rate as f64 / OUTPUT_RATE as f64;
            let left = (pos.floor() as usize).min(input.len() - 1);
            let right = (left + 1).min(input.len() - 1);
            let frac = (pos - left as f64) as f32;
            output.push(
                (input[left] as f32 * (1.0 - frac) + input[right] as f32 * frac).round() as i16,
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uncertain_chunk_keeps_the_page_language_and_route() {
        let mut page = AppConfig::default();
        page.speech.language = "ar".into();
        page.speech.selection.mode = SelectionMode::AutoDetect;
        let chunk = unit_route(&page, "123 🙂").unwrap();
        assert_eq!(chunk.speech.language, "ar");
        assert_eq!(chunk.speech.provider, page.speech.provider);
    }
    #[test]
    fn concatenates_rates_in_order_and_enforces_total_limit() {
        let mut output = Vec::new();
        append_pcm(&mut output, &[10, 20], 24_000, 8).unwrap();
        append_pcm(&mut output, &[30, 50], 12_000, 8).unwrap();
        assert_eq!(output, [10, 20, 30, 40, 50, 50]);
        assert!(append_pcm(&mut output, &[0; 3], 24_000, 8).is_err());
        assert_eq!(output.len(), 6);
    }
}
