//! Request-scoped cancellation and retry policy for the HTTP server.
//!
//! Deadline stages and their cold/warm limits come from
//! `reader_core::request_budget`; this wrapper adds a request identity and a
//! cancellation signal shared only by clones for that request. It deliberately
//! does not terminate workers. A caller must still target the owning job and
//! verify the exact worker incarnation before cancelling or terminating one.

use reader_core::request_budget::{BudgetExpiry, RequestBudget, RequestBudgets, RequestStage};
use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifetimeFailure {
    Cancelled,
    Deadline(BudgetExpiry),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifetimeError {
    request_id: String,
    failure: LifetimeFailure,
}

impl LifetimeError {}

impl fmt::Display for LifetimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.failure {
            LifetimeFailure::Cancelled => {
                write!(f, "request {} was cancelled", self.request_id)
            }
            LifetimeFailure::Deadline(reason) => {
                write!(
                    f,
                    "request {} exceeded its {reason:?} deadline",
                    self.request_id
                )
            }
        }
    }
}

impl Error for LifetimeError {}

#[derive(Debug)]
struct RequestState {
    id: String,
    started_at: Instant,
    budgets: RequestBudgets,
    budget: Mutex<RequestBudget>,
    cancelled: AtomicBool,
}

/// A request budget created at HTTP acceptance, before queueing.
///
/// Clones share the current stage and cancellation state. Use one instance for
/// the complete request/job lifetime so queue wait, loading, and inference
/// all consume the same absolute total budget.
#[derive(Debug, Clone)]
pub struct RequestLifetime {
    state: Arc<RequestState>,
}

impl RequestLifetime {
    pub fn new(request_id: impl Into<String>, cold_start: bool, budgets: RequestBudgets) -> Self {
        let started_at = Instant::now();
        Self {
            state: Arc::new(RequestState {
                id: request_id.into(),
                started_at,
                budgets,
                budget: Mutex::new(RequestBudget::new(started_at, cold_start, budgets)),
                cancelled: AtomicBool::new(false),
            }),
        }
    }

    pub fn request_id(&self) -> &str {
        &self.state.id
    }

    #[cfg(test)]
    pub fn started_at(&self) -> Instant {
        self.state.started_at
    }

    /// The absolute total deadline, irrespective of progress or stage changes.
    pub fn hard_deadline(&self) -> Instant {
        self.state.started_at + self.state.budgets.total
    }

    pub fn stage(&self) -> RequestStage {
        self.state
            .budget
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .stage()
    }

    /// Move the shared request into its next bounded phase.
    pub fn set_stage(&self, stage: RequestStage) {
        self.state
            .budget
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .set_stage(stage, Instant::now());
    }

    /// Record actual stream progress; this only advances stream inactivity.
    pub fn progress(&self) {
        self.state
            .budget
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .progress(Instant::now());
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.state
            .budget
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .deadline()
    }

    pub fn remaining(&self) -> Option<Duration> {
        if self.is_cancelled() {
            return None;
        }
        self.deadline()?.checked_duration_since(Instant::now())
    }

    pub fn expired(&self) -> Option<BudgetExpiry> {
        self.state
            .budget
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .expired(Instant::now())
    }

    pub fn check(&self) -> Result<(), LifetimeError> {
        if self.is_cancelled() {
            return Err(self.error(LifetimeFailure::Cancelled));
        }
        if let Some(reason) = self.expired() {
            return Err(self.error(LifetimeFailure::Deadline(reason)));
        }
        Ok(())
    }

    /// Finishes the shared budget so late observers do not report a timeout.
    pub fn finish(&self) {
        self.state
            .budget
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .finish();
    }

    /// Cancels this request and its clones only. It does not kill a worker.
    pub fn cancel(&self) {
        self.state.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire)
    }

    fn error(&self, failure: LifetimeFailure) -> LifetimeError {
        LifetimeError {
            request_id: self.state.id.clone(),
            failure,
        }
    }
}

/// Finite exponential retry policy. `max_attempts` includes the first attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    max_attempts: u32,
    initial_backoff: Duration,
    max_backoff: Duration,
}

impl RetryPolicy {
    pub const fn new(max_attempts: u32, initial_backoff: Duration, max_backoff: Duration) -> Self {
        Self {
            max_attempts,
            initial_backoff,
            max_backoff,
        }
    }

    /// One retry maximum, after 100 ms, for a total of two attempts.
    pub const fn default_network() -> Self {
        Self::new(2, Duration::from_millis(100), Duration::from_millis(500))
    }

    /// Returns a retry delay only for transport failures and only while both
    /// the attempt budget and current request phase deadline permit retry.
    /// `failed_attempt` is one-based and includes the attempt that just failed.
    pub fn delay_before_retry(
        self,
        failed_attempt: u32,
        lifetime: &RequestLifetime,
    ) -> Option<Duration> {
        if self.max_attempts == 0
            || failed_attempt == 0
            || failed_attempt >= self.max_attempts
            || lifetime.check().is_err()
        {
            return None;
        }

        let shift = failed_attempt.saturating_sub(1).min(31);
        let factor = 1u32 << shift;
        let proposed = self
            .initial_backoff
            .checked_mul(factor)
            .unwrap_or(self.max_backoff)
            .min(self.max_backoff);
        (lifetime.remaining()? > proposed).then_some(proposed)
    }
}

#[cfg(test)]
mod tests {
    use super::{LifetimeFailure, RequestLifetime, RetryPolicy};
    use reader_core::request_budget::{BudgetExpiry, RequestBudgets, RequestStage};
    use std::time::Duration;

    #[test]
    fn request_lifetime_shares_stages_and_hard_deadline_across_clones() {
        let budgets = RequestBudgets {
            total: Duration::from_secs(2),
            ..RequestBudgets::default()
        };
        let lifetime = RequestLifetime::new("job-a", true, budgets);
        let clone = lifetime.clone();
        let hard_deadline = lifetime.hard_deadline();
        clone.set_stage(RequestStage::Queued);

        assert_eq!(lifetime.stage(), RequestStage::Queued);
        assert_eq!(clone.hard_deadline(), hard_deadline);
    }

    #[test]
    fn cancellation_is_shared_only_by_clones_of_the_same_request() {
        let budgets = RequestBudgets::default();
        let a = RequestLifetime::new("request-a", false, budgets);
        let clone = a.clone();
        let b = RequestLifetime::new("request-b", false, budgets);
        a.cancel();

        assert!(clone.is_cancelled());
        assert_eq!(
            clone.check().unwrap_err().failure,
            LifetimeFailure::Cancelled
        );
        assert!(!b.is_cancelled());
        assert!(b.check().is_ok());
    }

    #[test]
    fn retry_budget_is_finite_and_phase_deadline_bounded() {
        let budgets = RequestBudgets {
            warm_inference: Duration::from_secs(1),
            ..RequestBudgets::default()
        };
        let lifetime = RequestLifetime::new("request-a", false, budgets);
        lifetime.set_stage(RequestStage::Inference);
        let policy = RetryPolicy::new(3, Duration::from_millis(10), Duration::from_millis(30));

        assert_eq!(
            policy.delay_before_retry(1, &lifetime),
            Some(Duration::from_millis(10))
        );
        assert_eq!(
            policy.delay_before_retry(2, &lifetime),
            Some(Duration::from_millis(20))
        );
        assert_eq!(policy.delay_before_retry(3, &lifetime), None);
    }

    #[test]
    fn retries_stop_after_cancel_or_stage_expiry() {
        let policy = RetryPolicy::default_network();
        let cancelled = RequestLifetime::new("cancelled", false, RequestBudgets::default());
        cancelled.cancel();
        assert_eq!(policy.delay_before_retry(1, &cancelled), None);

        let budgets = RequestBudgets {
            warm_inference: Duration::ZERO,
            ..RequestBudgets::default()
        };
        let expired = RequestLifetime::new("expired", false, budgets);
        expired.set_stage(RequestStage::Inference);
        assert_eq!(
            expired.check().unwrap_err().failure,
            LifetimeFailure::Deadline(BudgetExpiry::Inference)
        );
        assert_eq!(policy.delay_before_retry(1, &expired), None);
    }

    #[test]
    fn stream_progress_does_not_reset_absolute_hard_deadline() {
        let start = std::time::Instant::now();
        let budgets = RequestBudgets {
            stream_inactivity: Duration::from_secs(10),
            total: Duration::from_secs(3),
            ..RequestBudgets::default()
        };
        let lifetime = RequestLifetime::new("stream", false, budgets);
        lifetime.set_stage(RequestStage::Streaming);
        let hard_deadline = lifetime.hard_deadline();
        lifetime.progress();

        assert_eq!(lifetime.hard_deadline(), hard_deadline);
        assert!(lifetime.deadline().unwrap() <= hard_deadline);
        assert!(lifetime.started_at() >= start);
    }
}
