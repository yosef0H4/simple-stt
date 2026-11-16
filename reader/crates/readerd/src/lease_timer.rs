//! One wakeable timer per lease type; renewing a lease replaces its pending callback.
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

type Task = Box<dyn FnOnce() + Send + 'static>;
#[derive(Default)]
struct State {
    pending: Option<(Instant, Task)>,
    running: bool,
}
#[derive(Default)]
pub struct LeaseTimer {
    shared: Arc<(Mutex<State>, Condvar)>,
}
impl LeaseTimer {
    pub fn cancel(&self) {
        let (lock, wake) = &*self.shared;
        let mut state = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.pending = None;
        wake.notify_one();
    }

    pub fn schedule(&self, deadline: Instant, task: impl FnOnce() + Send + 'static) {
        let (lock, wake) = &*self.shared;
        let mut state = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.pending = Some((deadline, Box::new(task)));
        if !state.running {
            state.running = true;
            let shared = Arc::clone(&self.shared);
            std::thread::spawn(move || run(shared));
        }
        wake.notify_one();
    }
}
fn run(shared: Arc<(Mutex<State>, Condvar)>) {
    let (lock, wake) = &*shared;
    let mut state = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    loop {
        let Some((deadline, _)) = state.pending.as_ref() else {
            state.running = false;
            return;
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        if !remaining.is_zero() {
            state = wake
                .wait_timeout(state, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
            continue;
        }
        let (_, task) = state.pending.take().unwrap();
        drop(state);
        task();
        state = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn renewal_replaces_the_pending_callback_and_wakes_the_timer() {
        let timer = LeaseTimer::default();
        let (send, recv) = mpsc::channel();
        for value in 0..100 {
            let send = send.clone();
            timer.schedule(Instant::now() + Duration::from_secs(10), move || {
                send.send(value).unwrap()
            });
        }
        timer.schedule(Instant::now(), move || send.send(100).unwrap());
        assert_eq!(recv.recv_timeout(Duration::from_secs(2)).unwrap(), 100);
        assert!(recv.recv_timeout(Duration::from_millis(50)).is_err());
    }
}
