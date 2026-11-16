//! Monotonic, stage-aware request budgets shared by reader services.
//!
//! Progress only moves the streaming inactivity deadline. It never moves a
//! stage deadline or the absolute request deadline.
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Per-stage limits and the absolute cap for one request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RequestBudgets {
    #[serde(rename = "connect_timeout_secs", with = "duration_seconds")]
    pub connect: Duration,
    #[serde(rename = "queue_timeout_secs", with = "duration_seconds")]
    pub queue: Duration,
    #[serde(rename = "cold_model_load_timeout_secs", with = "duration_seconds")]
    pub cold_model_load: Duration,
    #[serde(rename = "warm_model_load_timeout_secs", with = "duration_seconds")]
    pub warm_model_load: Duration,
    #[serde(rename = "warm_inference_timeout_secs", with = "duration_seconds")]
    pub warm_inference: Duration,
    #[serde(rename = "stream_inactivity_timeout_secs", with = "duration_seconds")]
    pub stream_inactivity: Duration,
    #[serde(rename = "total_timeout_secs", with = "duration_seconds")]
    pub total: Duration,
}

mod duration_seconds {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(value: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(value.as_secs())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        u64::deserialize(deserializer).map(Duration::from_secs)
    }
}

impl Default for RequestBudgets {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(3),
            queue: Duration::from_secs(30),
            cold_model_load: Duration::from_secs(180),
            warm_model_load: Duration::from_secs(30),
            warm_inference: Duration::from_secs(60),
            stream_inactivity: Duration::from_secs(60),
            total: Duration::from_secs(300),
        }
    }
}

/// The bounded phase currently being performed for a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestStage {
    Connecting,
    Queued,
    LoadingModel,
    Inference,
    Streaming,
    Finished,
}

/// Which configured limit ended a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetExpiry {
    Connect,
    Queue,
    ModelLoad,
    Inference,
    StreamInactivity,
    Total,
}

/// A request's immutable start time plus its current phase and progress time.
#[derive(Clone, Debug)]
pub struct RequestBudget {
    started_at: Instant,
    stage_started_at: Instant,
    last_progress_at: Instant,
    stage: RequestStage,
    cold_start: bool,
    budgets: RequestBudgets,
}

impl RequestBudget {
    /// Start a request budget at `started_at`. The initial phase is Connecting.
    pub fn new(started_at: Instant, cold_start: bool, budgets: RequestBudgets) -> Self {
        Self {
            started_at,
            stage_started_at: started_at,
            last_progress_at: started_at,
            stage: RequestStage::Connecting,
            cold_start,
            budgets,
        }
    }

    /// Enter a phase. Only this phase's own timer starts over; the absolute
    /// deadline remains anchored to the original request start.
    pub fn set_stage(&mut self, stage: RequestStage, now: Instant) {
        self.stage = stage;
        self.stage_started_at = now;
        self.last_progress_at = now;
    }

    /// Record real forward progress. This affects only streaming inactivity.
    pub fn progress(&mut self, now: Instant) {
        self.last_progress_at = now;
    }

    pub fn stage(&self) -> RequestStage {
        self.stage
    }

    pub fn is_cold_start(&self) -> bool {
        self.cold_start
    }

    /// The next absolute monotonic deadline, including the hard total cap.
    /// Returns `None` after the request has finished.
    pub fn deadline(&self) -> Option<Instant> {
        if self.stage == RequestStage::Finished {
            return None;
        }
        let phase_deadline = match self.stage {
            RequestStage::Connecting => Some(self.stage_started_at + self.budgets.connect),
            RequestStage::Queued => Some(self.stage_started_at + self.budgets.queue),
            RequestStage::LoadingModel => {
                let duration = if self.cold_start {
                    self.budgets.cold_model_load
                } else {
                    self.budgets.warm_model_load
                };
                Some(self.stage_started_at + duration)
            }
            RequestStage::Inference => Some(self.stage_started_at + self.budgets.warm_inference),
            RequestStage::Streaming => Some(self.last_progress_at + self.budgets.stream_inactivity),
            RequestStage::Finished => None,
        };
        let total_deadline = self.started_at + self.budgets.total;
        Some(phase_deadline.map_or(total_deadline, |phase| phase.min(total_deadline)))
    }

    /// Return the expired limit, if any. The total cap wins when both limits
    /// expire at the same instant.
    pub fn expired(&self, now: Instant) -> Option<BudgetExpiry> {
        if self.stage == RequestStage::Finished {
            return None;
        }
        if now >= self.started_at + self.budgets.total {
            return Some(BudgetExpiry::Total);
        }
        let phase_expired = now
            >= self.stage_started_at
                + match self.stage {
                    RequestStage::Connecting => self.budgets.connect,
                    RequestStage::Queued => self.budgets.queue,
                    RequestStage::LoadingModel => {
                        if self.cold_start {
                            self.budgets.cold_model_load
                        } else {
                            self.budgets.warm_model_load
                        }
                    }
                    RequestStage::Inference => self.budgets.warm_inference,
                    RequestStage::Streaming => {
                        if now >= self.last_progress_at + self.budgets.stream_inactivity {
                            return Some(BudgetExpiry::StreamInactivity);
                        }
                        return None;
                    }
                    RequestStage::Finished => return None,
                };
        if !phase_expired {
            return None;
        }
        Some(match self.stage {
            RequestStage::Connecting => BudgetExpiry::Connect,
            RequestStage::Queued => BudgetExpiry::Queue,
            RequestStage::LoadingModel => BudgetExpiry::ModelLoad,
            RequestStage::Inference => BudgetExpiry::Inference,
            RequestStage::Streaming | RequestStage::Finished => return None,
        })
    }

    pub fn finish(&mut self) {
        self.stage = RequestStage::Finished;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn cold_load_has_grace_beyond_warm_inference_budget() {
        let start = t0();
        let budgets = RequestBudgets::default();
        let mut request = RequestBudget::new(start, true, budgets);
        request.set_stage(RequestStage::LoadingModel, start);

        assert_eq!(
            request.expired(start + Duration::from_secs(61)),
            None,
            "cold loading may take longer than the warm inference limit"
        );
        assert_eq!(
            request.expired(start + Duration::from_secs(180)),
            Some(BudgetExpiry::ModelLoad)
        );
    }

    #[test]
    fn stuck_cold_model_load_expires_at_its_own_limit() {
        let start = t0();
        let mut request = RequestBudget::new(start, true, RequestBudgets::default());
        request.set_stage(RequestStage::LoadingModel, start);

        assert_eq!(request.expired(start + Duration::from_secs(179)), None);
        assert_eq!(
            request.expired(start + Duration::from_secs(180)),
            Some(BudgetExpiry::ModelLoad)
        );
    }

    #[test]
    fn warm_and_cold_model_loads_use_distinct_limits() {
        let start = t0();
        let mut cold = RequestBudget::new(start, true, RequestBudgets::default());
        cold.set_stage(RequestStage::LoadingModel, start);
        let mut warm = RequestBudget::new(start, false, RequestBudgets::default());
        warm.set_stage(RequestStage::LoadingModel, start);

        assert_eq!(cold.expired(start + Duration::from_secs(31)), None);
        assert_eq!(
            warm.expired(start + Duration::from_secs(30)),
            Some(BudgetExpiry::ModelLoad)
        );
    }

    #[test]
    fn inference_uses_its_own_budget_after_model_loading() {
        let start = t0();
        let mut request = RequestBudget::new(start, true, RequestBudgets::default());
        request.set_stage(RequestStage::LoadingModel, start);
        let ready = start + Duration::from_secs(120);
        assert_eq!(request.expired(ready), None);
        request.set_stage(RequestStage::Inference, ready);

        assert_eq!(request.expired(ready + Duration::from_secs(59)), None);
        assert_eq!(
            request.expired(ready + Duration::from_secs(60)),
            Some(BudgetExpiry::Inference)
        );
    }

    #[test]
    fn progress_resets_stream_idle_only_and_never_the_total_cap() {
        let start = t0();
        let budgets = RequestBudgets {
            total: Duration::from_secs(20),
            stream_inactivity: Duration::from_secs(5),
            ..Default::default()
        };
        let mut request = RequestBudget::new(start, false, budgets);
        request.set_stage(RequestStage::Streaming, start);

        for second in [4, 8, 12, 16, 19] {
            request.progress(start + Duration::from_secs(second));
            assert_eq!(request.expired(start + Duration::from_secs(second)), None);
        }
        assert_eq!(
            request.expired(start + Duration::from_secs(20)),
            Some(BudgetExpiry::Total)
        );
    }

    #[test]
    fn phase_deadlines_and_connect_queue_limits_are_bounded() {
        let start = t0();
        let mut request = RequestBudget::new(start, false, RequestBudgets::default());
        assert_eq!(
            request.expired(start + Duration::from_secs(3)),
            Some(BudgetExpiry::Connect)
        );
        request.set_stage(RequestStage::Queued, start + Duration::from_secs(3));
        assert_eq!(
            request.expired(start + Duration::from_secs(33)),
            Some(BudgetExpiry::Queue)
        );
    }

    #[test]
    fn finished_requests_have_no_deadline_or_expiry() {
        let start = t0();
        let mut request = RequestBudget::new(start, true, RequestBudgets::default());
        request.finish();
        assert_eq!(request.deadline(), None);
        assert_eq!(request.expired(start + Duration::from_secs(600)), None);
    }
}
