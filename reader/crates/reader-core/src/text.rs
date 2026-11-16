use anyhow::{bail, Result};

pub const MAX_TEXT_BYTES: usize = 1_048_576;
pub const MAX_SENTENCE_BYTES: usize = 2_048;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeechUnit {
    pub sentence_index: usize,
    pub unit_index: usize,
    pub text: String,
    pub starts_in_code: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkProgress {
    /// Zero-based position in the planned speech-unit sequence.
    pub index: usize,
    pub total: usize,
    pub text: String,
}

pub fn chunk_progress(units: &[SpeechUnit], sentence: usize, unit: usize) -> Option<ChunkProgress> {
    let (index, current) = units.iter().enumerate().find(|(_, candidate)| {
        candidate.sentence_index == sentence && candidate.unit_index == unit
    })?;
    Some(ChunkProgress {
        index,
        total: units.len(),
        text: current.text.clone(),
    })
}

/// Render the concise native notice: current speech chunk followed by n/N.
pub fn format_chunk_progress(progress: &ChunkProgress) -> String {
    format!(
        "{} {}/{}",
        progress.text.replace('\0', "�"),
        progress.index + 1,
        progress.total
    )
}

pub fn normalize_source(text: &str) -> Result<String> {
    if text.len() > MAX_TEXT_BYTES {
        bail!("text exceeds the 1 MiB reading limit");
    }
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.trim().is_empty() {
        bail!("there is no readable text");
    }
    Ok(normalized)
}

/// Keep sentence identity stable while splitting long units near word
/// boundaries, so progress and replay remain sentence-oriented.
pub fn plan_units(text: &str, max_bytes: usize) -> Result<Vec<SpeechUnit>> {
    if !(64..=MAX_SENTENCE_BYTES).contains(&max_bytes) {
        bail!("speech unit limit must be 64 to 2048 bytes");
    }
    let text = normalize_source(text)?;
    let sentences = split_sentences(&text);
    let mut result = Vec::new();
    let mut in_code = false;
    for (sentence_index, sentence) in sentences.into_iter().enumerate() {
        let mut part = String::new();
        let mut unit_index = 0;
        for word in sentence.split_whitespace() {
            if word.len() > max_bytes {
                bail!("a word is longer than the speech-unit limit");
            }
            if !part.is_empty() && part.len() + 1 + word.len() > max_bytes {
                let text = std::mem::take(&mut part);
                let starts_in_code = in_code;
                in_code = crate::text_prep::code_after(&text, in_code);
                result.push(SpeechUnit {
                    sentence_index,
                    unit_index,
                    text,
                    starts_in_code,
                });
                unit_index += 1;
            }
            if !part.is_empty() {
                part.push(' ');
            }
            part.push_str(word);
        }
        if !part.is_empty() {
            let starts_in_code = in_code;
            in_code = crate::text_prep::code_after(&part, in_code);
            result.push(SpeechUnit {
                sentence_index,
                unit_index,
                text: part,
                starts_in_code,
            });
        }
    }
    if result.is_empty() {
        bail!("no speech units could be created");
    }
    Ok(result)
}

fn split_sentences(text: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (i, (index, ch)) in chars.iter().copied().enumerate() {
        if !matches!(ch, '.' | '!' | '?' | '。' | '！' | '？') {
            continue;
        }
        if ch == '.' {
            let previous = if i > 0 { chars[i - 1].1 } else { ' ' };
            let next = chars.get(i + 1).map(|(_, ch)| *ch).unwrap_or(' ');
            if previous.is_ascii_digit() && next.is_ascii_digit() {
                continue;
            }
            if !next.is_whitespace() && i + 1 < chars.len() {
                continue;
            }
            if is_abbreviation(&text[start..index + 1]) {
                continue;
            }
        }
        let mut end = index + ch.len_utf8();
        let mut look = i + 1;
        while let Some((byte, next)) = chars.get(look).copied() {
            if matches!(next, '"' | '\'' | ')' | ']' | '”' | '’' | '»') {
                end = byte + next.len_utf8();
                look += 1;
            } else {
                break;
            }
        }
        let candidate = text[start..end].trim();
        if !candidate.is_empty() {
            result.push(candidate)
        }
        start = end;
    }
    let tail = text[start..].trim();
    if !tail.is_empty() {
        result.push(tail);
    }
    result
}

fn is_abbreviation(prefix: &str) -> bool {
    let token = prefix
        .split_whitespace()
        .last()
        .unwrap_or("")
        .trim_matches(|c: char| matches!(c, '(' | '[' | '"' | '\''));
    matches!(
        token.to_ascii_lowercase().as_str(),
        "mr."
            | "mrs."
            | "ms."
            | "dr."
            | "prof."
            | "sr."
            | "jr."
            | "st."
            | "vs."
            | "etc."
            | "e.g."
            | "i.e."
            | "no."
            | "fig."
            | "inc."
            | "ltd."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_sentences_keep_sentence_ids_when_split() {
        let units = plan_units("One two three four five six. Next sentence!", 64).unwrap();
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].sentence_index, 0);
        assert_eq!(units[1].sentence_index, 1);
    }

    #[test]
    fn chunk_progress_counts_global_units_including_split_sentences() {
        let units = plan_units("One two three four five six. Next sentence!", 64).unwrap();
        let progress = chunk_progress(&units, 1, 0).unwrap();
        assert_eq!(progress.index, 1);
        assert_eq!(progress.total, 2);
        assert_eq!(progress.text, "Next sentence!");
        assert_eq!(format_chunk_progress(&progress), "Next sentence! 2/2");
    }

    #[test]
    fn chunk_progress_uses_each_long_sentence_unit_and_exact_text() {
        let units = plan_units(&format!("{}.", "word ".repeat(60)), 64).unwrap();
        assert!(units.len() > 2);
        let second = chunk_progress(&units, 0, 1).unwrap();
        assert_eq!(second.index, 1);
        assert_eq!(second.text, units[1].text);
        assert_eq!(
            format_chunk_progress(&second),
            format!("{} 2/{}", units[1].text, units.len())
        );
        assert!(chunk_progress(&units, 5, 0).is_none());
    }

    #[test]
    fn chunk_notice_replaces_embedded_nul_for_native_labels() {
        let progress = ChunkProgress {
            index: 0,
            total: 1,
            text: "hello\0world".into(),
        };
        assert_eq!(format_chunk_progress(&progress), "hello�world 1/1");
    }
    #[test]
    fn rejects_empty_and_oversized_input() {
        assert!(normalize_source(" \n ").is_err());
        assert!(normalize_source(&"x".repeat(MAX_TEXT_BYTES + 1)).is_err());
    }
    #[test]
    fn abbreviations_decimals_urls_and_newlines_keep_all_text() {
        let source = "Dr. Smith paid 3.14 dollars. Visit https://example.com/a.b.\nNext line!";
        let units = plan_units(source, 128).unwrap();
        let mut joined = String::new();
        for sentence in 0..3 {
            if sentence > 0 {
                joined.push(' ')
            }
            joined.push_str(
                &units
                    .iter()
                    .filter(|u| u.sentence_index == sentence)
                    .map(|u| u.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
            );
        }
        assert_eq!(
            joined,
            "Dr. Smith paid 3.14 dollars. Visit https://example.com/a.b. Next line!"
        );
        assert_eq!(units.iter().map(|u| u.sentence_index).max(), Some(2));
    }
}
