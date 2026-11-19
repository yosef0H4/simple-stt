//! Read-along page promotion and retry state.
//!
//! Capture and speech workers are asynchronous; this small state machine keeps
//! a late result from an older page or canceled read from being promoted.
use std::sync::atomic::{AtomicU64, Ordering};

/// OCR/TTS work already elapsed since the action must count toward settling.
pub fn settle_remaining(delay_ms: u16, elapsed: std::time::Duration) -> std::time::Duration {
    std::time::Duration::from_millis(u64::from(delay_ms)).saturating_sub(elapsed)
}

/// State retained across attempts for one pending page, never across promotion.
#[derive(Clone, Default)]
pub struct Preparation {
    advance_started: bool,
    pub text: Option<String>,
}
impl Preparation {
    pub fn new(advance_started: bool) -> Self {
        Self {
            advance_started,
            text: None,
        }
    }
    /// An uncertain input result must never cause the same action to be sent twice.
    pub fn begin_advance(&mut self) -> bool {
        !std::mem::replace(&mut self.advance_started, true)
    }
}

/// Retry processing without replaying input, with bounded, interruptible backoff.
pub fn retry<T>(
    limit: u8,
    current: &dyn Fn() -> bool,
    mut work: impl FnMut() -> anyhow::Result<T>,
    mut retrying: impl FnMut(u8),
) -> anyhow::Result<T> {
    for attempt in 0..=limit.min(5) {
        anyhow::ensure!(current(), "read-along was canceled");
        match work() {
            Ok(value) => {
                anyhow::ensure!(current(), "read-along was canceled");
                return Ok(value);
            }
            Err(error) if attempt == limit.min(5) || !current() => return Err(error),
            Err(_) => {
                retrying(attempt + 1);
                for _ in 0..20 {
                    anyhow::ensure!(current(), "read-along was canceled");
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        }
    }
    unreachable!()
}

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvanceDecision {
    WaitForAudio,
    WaitForPreparedPage,
    PromotePreparedPage,
    CaptureNextPage,
    PauseAfterAudio,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrDecision {
    RetryUnchanged,
    Prepared,
    PauseAfterAudio,
    Ignore,
}

#[derive(Debug, Clone)]
pub struct Controller {
    generation: u64,
    current_fingerprint: Option<u64>,
    prepared_fingerprint: Option<u64>,
    retry_count: u8,
    retry_limit: u8,
    capture_requested: bool,
    advance_requested: bool,
    audio_finished: bool,
    pause_after_audio: bool,
    active: bool,
}

impl Controller {
    pub fn new(retry_limit: u8) -> Self {
        Self {
            generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
            current_fingerprint: None,
            prepared_fingerprint: None,
            retry_count: 0,
            retry_limit: retry_limit.min(5),
            capture_requested: false,
            advance_requested: false,
            audio_finished: false,
            pause_after_audio: false,
            active: true,
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn accepts(&self, generation: u64) -> bool {
        self.active && self.generation == generation
    }

    pub fn failed(&self) -> bool {
        self.pause_after_audio
    }

    pub fn retry_failed(&mut self, generation: u64) -> bool {
        if !self.accepts(generation) || !self.pause_after_audio {
            return false;
        }
        self.pause_after_audio = false;
        self.capture_requested = false;
        self.prepared_fingerprint = None;
        self.retry_count = 0;
        true
    }

    pub fn cancel(&mut self) {
        self.active = false;
        self.generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        self.prepared_fingerprint = None;
        self.capture_requested = false;
        self.advance_requested = false;
    }

    pub fn set_current_page(&mut self, generation: u64, fingerprint: u64) -> bool {
        if !self.accepts(generation) {
            return false;
        }
        self.current_fingerprint = Some(fingerprint);
        self.prepared_fingerprint = None;
        self.retry_count = 0;
        self.capture_requested = false;
        self.advance_requested = false;
        self.audio_finished = false;
        self.pause_after_audio = false;
        true
    }

    /// Request a page advance. With prefetch disabled, audio completion gates
    /// even the first capture; with prefetch enabled, capture can proceed while
    /// the current page is still playing.
    pub fn request_advance(
        &mut self,
        generation: u64,
        prefetch: bool,
        audio_finished: bool,
    ) -> AdvanceDecision {
        if !self.accepts(generation) {
            return AdvanceDecision::Ignore;
        }
        self.retry_failed(generation);
        self.advance_requested = true;
        self.audio_finished |= audio_finished;
        if !prefetch && !self.audio_finished {
            return AdvanceDecision::WaitForAudio;
        }
        if self.prepared_fingerprint.is_some() {
            return if self.audio_finished {
                AdvanceDecision::PromotePreparedPage
            } else {
                AdvanceDecision::WaitForAudio
            };
        }
        if self.capture_requested {
            return AdvanceDecision::WaitForPreparedPage;
        }
        AdvanceDecision::CaptureNextPage
    }

    pub fn request_prefetch(&mut self, generation: u64) -> AdvanceDecision {
        if !self.accepts(generation) {
            return AdvanceDecision::Ignore;
        }
        if self.pause_after_audio {
            return if self.audio_finished {
                AdvanceDecision::PauseAfterAudio
            } else {
                AdvanceDecision::WaitForAudio
            };
        }
        if self.prepared_fingerprint.is_some() || self.capture_requested {
            return AdvanceDecision::WaitForPreparedPage;
        }
        self.capture_requested = true;
        AdvanceDecision::CaptureNextPage
    }

    pub fn audio_finished(&mut self, generation: u64) -> AdvanceDecision {
        if !self.accepts(generation) {
            return AdvanceDecision::Ignore;
        }
        self.audio_finished = true;
        if self.pause_after_audio {
            return AdvanceDecision::PauseAfterAudio;
        }
        if self.advance_requested && self.prepared_fingerprint.is_some() {
            return AdvanceDecision::PromotePreparedPage;
        }
        if self.advance_requested {
            return AdvanceDecision::CaptureNextPage;
        }
        AdvanceDecision::Ignore
    }

    pub fn ocr_result(&mut self, generation: u64, fingerprint: u64) -> OcrDecision {
        if !self.accepts(generation) || !self.capture_requested {
            return OcrDecision::Ignore;
        }
        if self.current_fingerprint == Some(fingerprint) {
            if self.retry_count < self.retry_limit {
                self.retry_count += 1;
                return OcrDecision::RetryUnchanged;
            }
            self.pause_after_audio = true;
            return OcrDecision::PauseAfterAudio;
        }
        self.retry_count = 0;
        self.capture_requested = false;
        self.prepared_fingerprint = Some(fingerprint);
        OcrDecision::Prepared
    }

    pub fn capture_failed(&mut self, generation: u64) -> AdvanceDecision {
        if !self.accepts(generation) {
            return AdvanceDecision::Ignore;
        }
        self.pause_after_audio = true;
        self.capture_requested = false;
        self.prepared_fingerprint = None;
        if self.audio_finished {
            AdvanceDecision::PauseAfterAudio
        } else {
            AdvanceDecision::WaitForAudio
        }
    }

    pub fn promote(&mut self, generation: u64) -> Option<u64> {
        if !self.accepts(generation) || !self.audio_finished {
            return None;
        }
        let fingerprint = self.prepared_fingerprint.take()?;
        self.current_fingerprint = Some(fingerprint);
        self.retry_count = 0;
        self.advance_requested = false;
        self.capture_requested = false;
        self.audio_finished = false;
        self.pause_after_audio = false;
        Some(fingerprint)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn five_retries_are_visible_and_then_stop() {
        let attempts = std::cell::Cell::new(0);
        let mut visible = Vec::new();
        let result: anyhow::Result<()> = super::retry(
            5,
            &|| true,
            || {
                attempts.set(attempts.get() + 1);
                anyhow::bail!("no audio")
            },
            |attempt| visible.push(attempt),
        );
        assert!(result.is_err());
        assert_eq!(attempts.get(), 6);
        assert_eq!(visible, [1, 2, 3, 4, 5]);
    }

    use super::{AdvanceDecision as Advance, Controller, OcrDecision};

    fn reading() -> (Controller, u64) {
        let mut controller = Controller::new(2);
        let generation = controller.generation();
        assert!(controller.set_current_page(generation, 10));
        (controller, generation)
    }

    #[test]
    fn transient_processing_failure_recovers_without_another_action() {
        let mut page = super::Preparation::default();
        assert!(page.begin_advance());
        let attempts = std::cell::Cell::new(0);
        let value = super::retry(
            2,
            &|| true,
            || {
                attempts.set(attempts.get() + 1);
                if attempts.get() == 1 {
                    anyhow::bail!("temporary transport failure");
                }
                Ok("ready")
            },
            |_| {},
        )
        .unwrap();
        assert_eq!(value, "ready");
        assert_eq!(attempts.get(), 2);
        assert!(!page.begin_advance());
    }
    #[test]
    fn processing_retries_are_bounded_and_stop_interrupts_backoff() {
        let attempts = std::cell::Cell::new(0);
        let result: anyhow::Result<()> = super::retry(
            1,
            &|| true,
            || {
                attempts.set(attempts.get() + 1);
                anyhow::bail!("offline")
            },
            |_| {},
        );
        assert!(result.is_err());
        assert_eq!(attempts.get(), 2);
        let current = std::cell::Cell::new(true);
        let attempts = std::cell::Cell::new(0);
        let result: anyhow::Result<()> = super::retry(
            5,
            &|| current.get(),
            || {
                attempts.set(attempts.get() + 1);
                anyhow::bail!("offline")
            },
            |_| current.set(false),
        );
        assert!(result.is_err());
        assert_eq!(attempts.get(), 1);
    }

    #[test]
    fn explicit_next_leaves_the_worker_free_to_claim_the_capture() {
        let (mut controller, generation) = reading();
        assert_eq!(
            controller.request_advance(generation, true, false),
            Advance::CaptureNextPage
        );
        assert_eq!(
            controller.request_prefetch(generation),
            Advance::CaptureNextPage
        );
    }

    #[test]
    fn explicit_next_can_recover_after_a_failed_page() {
        let (mut controller, generation) = reading();
        assert_eq!(
            controller.request_prefetch(generation),
            Advance::CaptureNextPage
        );
        assert_eq!(controller.capture_failed(generation), Advance::WaitForAudio);
        assert_eq!(
            controller.audio_finished(generation),
            Advance::PauseAfterAudio
        );
        assert_eq!(
            controller.request_advance(generation, true, true),
            Advance::CaptureNextPage
        );
        assert_eq!(
            controller.request_prefetch(generation),
            Advance::CaptureNextPage
        );
        assert_eq!(controller.ocr_result(generation, 20), OcrDecision::Prepared);
        assert_eq!(
            controller.audio_finished(generation),
            Advance::PromotePreparedPage
        );
        assert_eq!(controller.promote(generation), Some(20));
    }

    #[test]
    fn one_next_page_waits_for_current_playback_even_with_repeated_requests() {
        let (mut controller, generation) = reading();
        assert_eq!(
            controller.request_advance(generation, true, false),
            Advance::CaptureNextPage
        );
        assert_eq!(
            controller.request_prefetch(generation),
            Advance::CaptureNextPage
        );
        for _ in 0..20 {
            assert_eq!(
                controller.request_advance(generation, true, false),
                Advance::WaitForPreparedPage
            );
            assert_eq!(
                controller.request_prefetch(generation),
                Advance::WaitForPreparedPage
            );
        }
        assert_eq!(controller.ocr_result(generation, 20), OcrDecision::Prepared);
        for _ in 0..20 {
            assert_eq!(
                controller.request_advance(generation, true, false),
                Advance::WaitForAudio
            );
            assert_eq!(
                controller.request_prefetch(generation),
                Advance::WaitForPreparedPage
            );
            assert_eq!(controller.promote(generation), None);
        }
        assert_eq!(
            controller.audio_finished(generation),
            Advance::PromotePreparedPage
        );
        assert_eq!(controller.promote(generation), Some(20));
        assert_eq!(
            controller.request_prefetch(generation),
            Advance::CaptureNextPage
        );
    }

    #[test]
    fn settling_overlaps_processing_instead_of_restarting_after_it() {
        use std::time::Duration;
        assert_eq!(
            super::settle_remaining(900, Duration::from_millis(700)),
            Duration::from_millis(200)
        );
        assert_eq!(
            super::settle_remaining(900, Duration::from_millis(1200)),
            Duration::ZERO
        );
        assert_eq!(super::settle_remaining(0, Duration::ZERO), Duration::ZERO);
    }
    #[test]
    fn prefetch_waits_for_promotion_until_current_audio_finishes() {
        let (mut controller, generation) = reading();
        assert_eq!(
            controller.request_advance(generation, true, false),
            Advance::CaptureNextPage
        );
        assert_eq!(
            controller.request_prefetch(generation),
            Advance::CaptureNextPage
        );
        assert_eq!(controller.ocr_result(generation, 11), OcrDecision::Prepared);
        assert_eq!(
            controller.request_advance(generation, true, false),
            Advance::WaitForAudio
        );
        assert_eq!(
            controller.audio_finished(generation),
            Advance::PromotePreparedPage
        );
        assert_eq!(controller.promote(generation), Some(11));
    }

    #[test]
    fn disabling_prefetch_delays_capture_until_audio_finishes() {
        let (mut controller, generation) = reading();
        assert_eq!(
            controller.request_advance(generation, false, false),
            Advance::WaitForAudio
        );
        assert_eq!(
            controller.audio_finished(generation),
            Advance::CaptureNextPage
        );
    }

    #[test]
    fn unchanged_ocr_retries_are_bounded_then_pause_after_audio() {
        let (mut controller, generation) = reading();
        assert_eq!(
            controller.request_advance(generation, true, false),
            Advance::CaptureNextPage
        );
        assert_eq!(
            controller.request_prefetch(generation),
            Advance::CaptureNextPage
        );
        assert_eq!(
            controller.ocr_result(generation, 10),
            OcrDecision::RetryUnchanged
        );
        assert_eq!(
            controller.ocr_result(generation, 10),
            OcrDecision::RetryUnchanged
        );
        assert_eq!(
            controller.ocr_result(generation, 10),
            OcrDecision::PauseAfterAudio
        );
        assert_eq!(
            controller.audio_finished(generation),
            Advance::PauseAfterAudio
        );
    }

    #[test]
    fn lost_target_and_capture_failure_finish_current_audio_then_pause() {
        let (mut controller, generation) = reading();
        assert_eq!(controller.capture_failed(generation), Advance::WaitForAudio);
        assert_eq!(
            controller.audio_finished(generation),
            Advance::PauseAfterAudio
        );
    }

    #[test]
    fn cancellation_fences_late_capture_and_ocr_results() {
        let (mut controller, generation) = reading();
        controller.cancel();
        assert_eq!(controller.ocr_result(generation, 11), OcrDecision::Ignore);
        assert_eq!(controller.capture_failed(generation), Advance::Ignore);
        assert!(!controller.accepts(generation));
    }
}
