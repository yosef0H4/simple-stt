//! Bounded alternate sizing for dense Arabic PP-OCRv5 lines.
//!
//! Detection, sorting, and perspective crops use the same public OAR
//! components as its standard pipeline. Small batches bound padding and memory.
//! Arabic lines may
//! use a wider input when that improves the model confidence; Latin lines keep
//! their ordinary sizing. This uses the same two sessions, not another model.
//!
//! Model-specific rationale: PaddleOCR issue #18349 describes inadequate CTC
//! time steps for dense Arabic at the ordinary aspect ratio. See the research
//! note in docs/research/arabic-screenshot-ocr.md and the independent corpus
//! comparison. Other recognizers and detector presets retain OAR's pipeline.
use std::path::Path;
use std::sync::Arc;

use anyhow::{ensure, Context, Result};
use image::{imageops::FilterType, RgbImage};
use oar_ocr::core::config::OrtSessionConfig;
use oar_ocr::core::traits::{AdapterBuilder, ImageTaskInput, ModelAdapter, OrtConfigurable};
use oar_ocr::domain::adapters::{
    TextDetectionAdapter, TextDetectionAdapterBuilder, TextRecognitionAdapter,
    TextRecognitionAdapterBuilder,
};
use oar_ocr::oarocr::{EdgeProcessor, TextCroppingProcessor};
use oar_ocr::processors::sort_quad_boxes;
use reader_core::paddle::PaddleProfile;

const HEIGHT: u32 = 48;
const MIN_WIDTH: u32 = 320;
const MAX_WIDTH: u32 = 3200;
const BATCH_SIZE: usize = 6;
// RapidOCR's default acceptance threshold avoids reading low-confidence icons
// and decoration as text. Apply it after the alternate pass can recover a line.
const MIN_CONFIDENCE: f32 = 0.5;

/// Round twice the natural width at recognition height, then bound it.
fn widened_width(width: u32, height: u32) -> Option<u32> {
    if width == 0 || height == 0 {
        return None;
    }
    let scaled =
        (u64::from(width) * u64::from(HEIGHT) * 2 + u64::from(height) / 2) / u64::from(height);
    Some(scaled.clamp(u64::from(MIN_WIDTH), u64::from(MAX_WIDTH)) as u32)
}

pub struct ArabicPipeline {
    detection: TextDetectionAdapter,
    recognition: TextRecognitionAdapter,
}

impl ArabicPipeline {
    /// Scope this model-specific sizing policy to the Arabic PP-OCRv5 bundle.
    pub fn new(model_dir: &Path, profile: &PaddleProfile, ort: OrtSessionConfig) -> Result<Self> {
        ensure!(
            profile.recognizer == "arabic_pp-ocrv5_mobile_rec.onnx",
            "Arabic width policy requires the Arabic PP-OCRv5 model"
        );
        let dictionary = std::fs::read_to_string(model_dir.join(&profile.dictionary))
            .context("reading OCR character dictionary")?;
        let detection = TextDetectionAdapterBuilder::new()
            .with_ort_config(ort.clone())
            .build(model_dir.join(&profile.detector))
            .map_err(|error| anyhow::anyhow!("loading OCR detector: {error}"))?;
        let recognition = TextRecognitionAdapterBuilder::new()
            .character_dict(dictionary.lines().map(str::to_owned).collect())
            .with_ort_config(ort)
            .build(model_dir.join(&profile.recognizer))
            .map_err(|error| anyhow::anyhow!("loading OCR recognizer: {error}"))?;
        Ok(Self {
            detection,
            recognition,
        })
    }

    /// Retain standard detection order and bound recognition to two attempts per line.
    /// Arabic text direction is handled by the caller's existing postprocessor.
    pub fn predict(&self, image: RgbImage) -> Result<Vec<String>> {
        let image = Arc::new(image);
        let detected = self
            .detection
            .execute(
                ImageTaskInput::from_arc_images(vec![Arc::clone(&image)]),
                None,
            )
            .map_err(|error| anyhow::anyhow!("detecting OCR regions: {error}"))?;
        ensure!(
            detected.detections.len() == 1,
            "unexpected OCR detection result count"
        );
        let boxes = detected
            .detections
            .into_iter()
            .next()
            .unwrap()
            .into_iter()
            .map(|d| d.bbox)
            .collect::<Vec<_>>();
        let boxes = sort_quad_boxes(&boxes);
        let cropper = TextCroppingProcessor::new(true);
        let mut texts = Vec::with_capacity(boxes.len());
        for chunk in boxes.chunks(BATCH_SIZE) {
            // Crop only one bounded group at a time and fail visibly rather
            // than silently dropping an invalid detected region.
            let crops = cropper
                .process((Arc::clone(&image), chunk.to_vec()))
                .map_err(|error| anyhow::anyhow!("cropping OCR region: {error}"))?;
            ensure!(
                crops.len() == chunk.len(),
                "unexpected OCR crop result count"
            );
            let crops = crops
                .into_iter()
                .map(|crop| crop.context("could not crop detected OCR region"))
                .collect::<Result<Vec<_>>>()?;
            let mut output = self
                .recognition
                .execute(ImageTaskInput::from_arc_images(crops.clone()), None)
                .map_err(|error| anyhow::anyhow!("recognizing OCR region: {error}"))?;
            ensure!(
                output.texts.len() == crops.len() && output.scores.len() == crops.len(),
                "unexpected OCR recognition result count"
            );
            let mut candidate_indices = Vec::new();
            let mut candidate_crops = Vec::new();
            for (index, crop) in crops.iter().enumerate() {
                if output.scores[index] < 0.98
                    && u64::from(crop.width()) > u64::from(crop.height()) * 10
                    && output.texts[index]
                        .chars()
                        .any(|c| matches!(c as u32, 0x0600..=0x06ff))
                {
                    let width =
                        widened_width(crop.width(), crop.height()).context("empty OCR region")?;
                    let resized =
                        image::imageops::resize(crop.as_ref(), width, HEIGHT, FilterType::Triangle);
                    candidate_indices.push(index);
                    candidate_crops.push(Arc::new(resized));
                }
            }
            if !candidate_crops.is_empty() {
                let wider = self
                    .recognition
                    .execute(ImageTaskInput::from_arc_images(candidate_crops), None)
                    .map_err(|error| anyhow::anyhow!("recognizing Arabic OCR region: {error}"))?;
                ensure!(
                    wider.texts.len() == candidate_indices.len()
                        && wider.scores.len() == candidate_indices.len(),
                    "unexpected Arabic OCR result count"
                );
                for (candidate, (text, score)) in candidate_indices
                    .into_iter()
                    .zip(wider.texts.into_iter().zip(wider.scores))
                {
                    if score > output.scores[candidate] {
                        output.texts[candidate] = text;
                        output.scores[candidate] = score;
                    }
                }
            }
            texts.extend(
                output
                    .texts
                    .into_iter()
                    .zip(output.scores)
                    .filter(|(_, score)| *score >= MIN_CONFIDENCE)
                    .map(|(text, _)| text),
            );
        }
        Ok(texts)
    }
}

#[cfg(test)]
mod tests {
    use super::widened_width;

    #[test]
    fn line_width_is_doubled_at_height_48_and_bounded() {
        assert_eq!(widened_width(100, 48), Some(320));
        assert_eq!(widened_width(500, 48), Some(1000));
        assert_eq!(widened_width(1800, 110), Some(1571));
        assert_eq!(widened_width(501, 96), Some(501));
    }

    #[test]
    fn zero_dimensions_and_large_dimensions_are_safe() {
        assert_eq!(widened_width(0, 48), None);
        assert_eq!(widened_width(48, 0), None);
        assert_eq!(widened_width(u32::MAX, 1), Some(3200));
    }
}
