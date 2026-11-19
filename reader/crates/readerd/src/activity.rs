use serde::Serialize;
use std::collections::VecDeque;

#[derive(Clone, Serialize)]
pub struct Event {
    pub generation: u64,
    pub phase: &'static str,
    pub kind: &'static str,
}

#[derive(Clone, Serialize)]
pub struct Activity {
    pub generation: u64,
    pub phase: &'static str,
    pub message_id: &'static str,
    pub language: Option<String>,
    pub model: Option<String>,
    pub runtime: Option<String>,
    pub state: &'static str,
    pub events: VecDeque<Event>,
    #[serde(skip)]
    pub prepared_text: Option<String>,
    #[serde(skip)]
    pub before_pause: &'static str,
    #[serde(skip)]
    pub recover_read_along: bool,
}

impl Default for Activity {
    fn default() -> Self {
        Self {
            generation: 0,
            phase: "idle",
            message_id: "idle",
            language: None,
            model: None,
            runtime: None,
            state: "idle",
            events: VecDeque::new(),
            prepared_text: None,
            before_pause: "synthesizing",
            recover_read_along: false,
        }
    }
}

impl Activity {
    pub fn begin(&mut self, generation: u64) {
        if generation <= self.generation {
            return;
        }
        self.record("end");
        self.generation = generation;
        self.phase = "idle";
        self.message_id = "idle";
        self.language = None;
        self.model = None;
        self.runtime = None;
        self.state = "idle";
        self.prepared_text = None;
        self.before_pause = "synthesizing";
        self.recover_read_along = false;
        self.record("start");
    }
    pub fn set_prepared_text(&mut self, generation: u64, text: String) {
        if self.generation == generation {
            self.prepared_text = Some(text);
        }
    }
    pub fn set(&mut self, generation: u64, phase: &'static str) {
        if self.generation != generation || self.phase == phase {
            return;
        }
        self.record("end");
        self.phase = phase;
        self.message_id = match phase {
            "acquiring_selection" => "reader.phase.acquiring_selection",
            "selecting_region" => "reader.phase.selecting_region",
            "detecting_language" => "reader.phase.detecting_language",
            "recognizing" => "reader.phase.recognizing",
            "copying" => "reader.phase.copying",
            "loading_voice" => "reader.phase.loading_voice",
            "synthesizing" => "reader.phase.synthesizing",
            "reading" => "reader.phase.reading",
            "paused" => "reader.phase.paused",
            "error" => "reader.phase.error",
            _ => "idle",
        };
        self.record("start");
    }
    fn record(&mut self, kind: &'static str) {
        if self.events.len() == 16 {
            self.events.pop_front();
        }
        self.events.push_back(Event {
            generation: self.generation,
            phase: self.phase,
            kind,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::Activity;
    #[test]
    fn stale_generations_cannot_replace_current_phase_and_journal_is_bounded() {
        let mut activity = Activity::default();
        activity.begin(1);
        activity.set(1, "detecting_language");
        activity.begin(2);
        activity.set_prepared_text(1, "stale".into());
        assert!(activity.prepared_text.is_none());
        activity.set_prepared_text(2, "current".into());
        assert_eq!(activity.prepared_text.as_deref(), Some("current"));
        activity.set(2, "selecting_region");
        activity.set(1, "recognizing");
        activity.begin(1);
        assert_eq!(activity.phase, "selecting_region");
        for _ in 0..20 {
            activity.set(2, "recognizing");
            activity.set(2, "copying");
        }
        assert_eq!(activity.events.len(), 16);
        assert_eq!(activity.events.back().unwrap().kind, "start");
    }
}
