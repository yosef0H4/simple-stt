use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JobKey {
    pub id: Uuid,
    pub generation: u64,
    pub config_revision: u64,
}
impl JobKey {
    pub fn fresh(generation: u64, config_revision: u64) -> Self {
        Self {
            id: Uuid::new_v4(),
            generation,
            config_revision,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkerIncarnation(pub Uuid);
impl WorkerIncarnation {
    pub fn fresh() -> Self {
        Self(Uuid::new_v4())
    }
}

/// The callback checks the epoch before accepting PCM. Advancing it is the
/// synchronous stop/replacement boundary; worker shutdown can follow later.
#[derive(Debug, Default)]
pub struct GenerationGate(AtomicU64);
impl GenerationGate {
    pub fn current(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }
    pub fn advance(&self) -> u64 {
        self.0.fetch_add(1, Ordering::AcqRel).wrapping_add(1)
    }
    pub fn advance_from(&self, expected: u64) -> Option<u64> {
        let next = expected.wrapping_add(1);
        self.0
            .compare_exchange(expected, next, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| next)
    }
    pub fn accepts(&self, generation: u64) -> bool {
        self.current() == generation
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleAction {
    Launch,
    Reuse,
    Replace,
    Retire,
    KeepWarm,
}

#[derive(Debug, Default)]
pub struct WorkerLeaseState {
    pub incarnation: Option<WorkerIncarnation>,
    pub config_fingerprint: Option<String>,
    pub last_used_ms: Option<u64>,
}
impl WorkerLeaseState {
    pub fn before_job(&mut self, fingerprint: &str) -> LifecycleAction {
        match (self.incarnation, self.config_fingerprint.as_deref()) {
            (None, _) => {
                self.incarnation = Some(WorkerIncarnation::fresh());
                self.config_fingerprint = Some(fingerprint.into());
                LifecycleAction::Launch
            }
            (Some(_), Some(current)) if current != fingerprint => {
                self.incarnation = Some(WorkerIncarnation::fresh());
                self.config_fingerprint = Some(fingerprint.into());
                LifecycleAction::Replace
            }
            (Some(_), _) => LifecycleAction::Reuse,
        }
    }
    pub fn idle_action(&self, now_ms: u64, grace_ms: u64) -> Option<LifecycleAction> {
        self.incarnation?;
        self.last_used_ms
            .filter(|last| now_ms.saturating_sub(*last) >= grace_ms)
            .map(|_| LifecycleAction::Retire)
    }
    pub fn touched(&mut self, now_ms: u64) {
        self.last_used_ms = Some(now_ms);
    }
    pub fn stopped(&mut self) {
        self.incarnation = None;
        self.config_fingerprint = None;
        self.last_used_ms = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stop_epoch_rejects_old_worker_audio() {
        let gate = GenerationGate::default();
        let old = gate.current();
        assert!(gate.accepts(old));
        gate.advance();
        assert!(!gate.accepts(old));
    }
    #[test]
    fn conditional_advance_does_not_overwrite_a_newer_job() {
        let gate = GenerationGate::default();
        let expected = gate.current();
        assert_eq!(gate.advance_from(expected), Some(expected + 1));
        assert_eq!(gate.advance_from(expected), None);
        assert_eq!(gate.current(), expected + 1);
    }
    #[test]
    fn model_change_gets_a_new_worker_identity() {
        let mut lease = WorkerLeaseState::default();
        assert_eq!(lease.before_job("cpu-en"), LifecycleAction::Launch);
        let first = lease.incarnation;
        assert_eq!(lease.before_job("cpu-en"), LifecycleAction::Reuse);
        assert_eq!(lease.before_job("cuda-en"), LifecycleAction::Replace);
        assert_ne!(first, lease.incarnation);
    }
}
