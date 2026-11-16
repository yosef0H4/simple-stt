use serde::Serialize;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

#[derive(Clone, Serialize)]
pub struct ModelEvent {
    pub sequence: u64,
    pub happened_unix_ms: u64,
    pub feature: &'static str,
    pub model: String,
    pub runtime: String,
    pub language: String,
    pub event: &'static str,
}
#[derive(Default)]
struct Log {
    sequence: u64,
    entries: VecDeque<ModelEvent>,
}
#[derive(Clone, Default)]
pub struct ModelEvents(Arc<Mutex<Log>>);
impl ModelEvents {
    pub fn record(
        &self,
        feature: &'static str,
        model: &str,
        runtime: &str,
        language: &str,
        event: &'static str,
    ) {
        let mut log = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        log.sequence = log.sequence.saturating_add(1);
        let sequence = log.sequence;
        if log.entries.len() == 16 {
            log.entries.pop_front();
        }
        let happened_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        log.entries.push_back(ModelEvent {
            sequence,
            happened_unix_ms,
            feature,
            model: model.into(),
            runtime: runtime.into(),
            language: language.into(),
            event,
        });
    }
    pub fn snapshot(&self) -> Vec<ModelEvent> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entries
            .iter()
            .cloned()
            .collect()
    }
}
pub fn identity(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 128
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
        .unwrap_or("unknown")
        .into()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_sequence_preserves_model_identity_and_order() {
        let events = ModelEvents::default();
        for _ in 0..17 {
            events.record("tts", "supertonic3", "sherpa-cpu", "ar", "unloaded");
        }
        events.record("tts", "kokoro", "sherpa-cpu", "en", "loaded");
        let snapshot = events.snapshot();
        assert_eq!(snapshot.len(), 16);
        assert_eq!(snapshot[0].sequence, 3);
        assert_eq!(snapshot[15].model, "kokoro");
        assert_eq!(snapshot[15].language, "en");
        assert_eq!(snapshot[15].sequence, 18);
    }
}
