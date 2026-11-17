//! Volatile state for repeating the most recent explicit reading action.
//!
//! Text and window targets stay in memory only. Captured screenshots are never
//! retained by this module.

use crate::read_along_capture::Target;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionAction {
    Read,
    Copy,
}

#[derive(Debug, Clone)]
pub enum RepeatPayload {
    Text {
        source: String,
        text: String,
    },
    Region {
        action: RegionAction,
        target: Target,
    },
    ReadAlong {
        target: Target,
    },
    Unavailable {
        reason: String,
    },
}

#[derive(Debug, Clone, Default)]
pub struct RepeatActionState {
    last_action_at_ms: u64,
    last_action_kind: String,
    revision: u64,
    payload: Option<RepeatPayload>,
}

impl RepeatActionState {
    /// Begin an explicit action. Older asynchronous completions cannot replace
    /// the payload for a newer action because they carry this revision.
    pub fn begin(&mut self, kind: impl Into<String>, at_ms: u64) -> u64 {
        self.revision = self.revision.wrapping_add(1).max(1);
        self.last_action_at_ms = at_ms.max(self.last_action_at_ms.saturating_add(1));
        self.last_action_kind = kind.into();
        self.payload = None;
        self.revision
    }

    pub fn complete(&mut self, revision: u64, payload: RepeatPayload) {
        if self.revision == revision {
            if matches!(&payload, RepeatPayload::Unavailable { .. }) {
                self.last_action_kind = "unavailable-repeat".into();
            }
            self.payload = Some(payload);
        }
    }

    pub fn has_read_along_target(&self) -> bool {
        matches!(self.payload, Some(RepeatPayload::ReadAlong { .. }))
    }

    pub fn metadata(&self) -> (u64, String) {
        (self.last_action_at_ms, self.last_action_kind.clone())
    }

    /// Record an explicit Repeat invocation without changing its payload.
    pub fn repeat(&mut self, at_ms: u64) -> anyhow::Result<(String, RepeatPayload)> {
        let payload = self
            .payload
            .clone()
            .context("there is no completed reader action to repeat")?;
        self.last_action_at_ms = at_ms.max(self.last_action_at_ms.saturating_add(1));
        Ok((self.last_action_kind.clone(), payload))
    }
}

use anyhow::Context;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_read_along_keeps_the_saved_area_across_invocations() {
        let target = Target {
            window: crate::read_along_capture::WindowId::X11(77),
            region: crate::read_along_capture::Rect {
                x: 2,
                y: 3,
                width: 400,
                height: 200,
            },
        };
        let mut state = RepeatActionState::default();
        let revision = state.begin("read-along", 1);
        state.complete(
            revision,
            RepeatPayload::ReadAlong {
                target: target.clone(),
            },
        );
        for at in [2, 3] {
            match state.repeat(at).unwrap().1 {
                RepeatPayload::ReadAlong { target: saved } => {
                    assert_eq!(saved.window, target.window);
                    assert_eq!(saved.region, target.region);
                }
                _ => panic!("repeat must retain a capture target rather than cached speech"),
            }
        }
    }
    #[test]
    fn metadata_is_available_while_explicit_action_is_pending() {
        let mut state = RepeatActionState::default();
        state.begin("read-region", 1234);
        assert_eq!(state.metadata(), (1234, "read-region".into()));
        assert!(state.repeat(1235).is_err());
    }

    #[test]
    fn stale_action_completion_cannot_replace_newer_payload() {
        let mut state = RepeatActionState::default();
        let old = state.begin("read-text", 1);
        let current = state.begin("clipboard", 2);
        state.complete(
            old,
            RepeatPayload::Text {
                source: "old".into(),
                text: "old private text".into(),
            },
        );
        state.complete(
            current,
            RepeatPayload::Text {
                source: "clipboard".into(),
                text: "current text".into(),
            },
        );
        let (_, payload) = state.repeat(3).unwrap();
        assert!(matches!(payload, RepeatPayload::Text { text, .. } if text == "current text"));
        assert_eq!(state.metadata(), (3, "clipboard".into()));
    }

    #[test]
    fn unavailable_repeat_has_an_explicit_status_kind() {
        let mut state = RepeatActionState::default();
        let revision = state.begin("read-region", 5);
        state.complete(
            revision,
            RepeatPayload::Unavailable {
                reason: "no window target".into(),
            },
        );
        assert_eq!(state.metadata(), (5, "unavailable-repeat".into()));
    }

    #[test]
    fn explicit_actions_get_strictly_increasing_timestamps() {
        let mut state = RepeatActionState::default();
        let first = state.begin("clipboard", 900);
        state.complete(
            first,
            RepeatPayload::Text {
                source: "clipboard".into(),
                text: "hello".into(),
            },
        );
        let _second = state.begin("selection", 900);
        assert_eq!(state.metadata(), (901, "selection".into()));
        state.repeat(900).unwrap_err();
        assert_eq!(state.metadata(), (901, "selection".into()));
    }
}
