//! A pending page retains one queued chunk and one in-flight synthesis.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::time::Duration;
/// Completed synthesis per planned unit; playback has its own independent cursor.
#[derive(Debug)]
pub struct Progress(Vec<AtomicBool>);
impl Progress {
    pub fn new(total: usize) -> Self {
        Self((0..total).map(|_| AtomicBool::new(false)).collect())
    }
    pub fn mark(&self, index: usize) {
        if let Some(unit) = self.0.get(index) {
            unit.store(true, Ordering::Release);
        }
    }
    pub fn total(&self) -> usize {
        self.0.len()
    }
    pub fn generated(&self) -> usize {
        self.0
            .iter()
            .filter(|unit| unit.load(Ordering::Acquire))
            .count()
    }
    pub fn ready_after(&self, current: usize) -> usize {
        self.0
            .iter()
            .skip(current.saturating_add(1))
            .filter(|unit| unit.load(Ordering::Acquire))
            .count()
    }
}
pub struct Receiver<T> {
    rx: mpsc::Receiver<T>,
    alive: Arc<AtomicBool>,
}
pub struct Sender<T> {
    tx: mpsc::SyncSender<T>,
    alive: Arc<AtomicBool>,
}
pub fn tags(unit: &reader_core::text::SpeechUnit) -> (u32, u32) {
    (unit.sentence_index as u32, unit.unit_index as u32)
}
pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let (tx, rx) = mpsc::sync_channel(1);
    let alive = Arc::new(AtomicBool::new(true));
    (
        Sender {
            tx,
            alive: alive.clone(),
        },
        Receiver { rx, alive },
    )
}
impl<T> Sender<T> {
    pub fn active(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }
    pub fn send(&self, mut value: T, current: &dyn Fn() -> bool) -> bool {
        while self.active() && current() {
            match self.tx.try_send(value) {
                Ok(()) => return true,
                Err(mpsc::TrySendError::Full(returned)) => value = returned,
                Err(mpsc::TrySendError::Disconnected(_)) => return false,
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }
}
impl<T> Receiver<T> {
    pub fn receive(&self, current: &dyn Fn() -> bool) -> anyhow::Result<Option<T>> {
        loop {
            anyhow::ensure!(current(), "chunk playback was canceled");
            match self.rx.recv_timeout(Duration::from_millis(10)) {
                Ok(value) => return Ok(Some(value)),
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(None),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
}
impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn prepared_ahead_count_decreases_with_playback_and_survives_promotion() {
        let progress = std::sync::Arc::new(super::Progress::new(6));
        for index in [0, 1, 2] {
            progress.mark(index);
        }
        assert_eq!(progress.generated(), 3);
        let promoted = progress.clone();
        assert_eq!(promoted.ready_after(0), 2);
        assert_eq!(promoted.ready_after(1), 1);
        assert_eq!(promoted.ready_after(2), 0);
        progress.mark(3);
        assert_eq!(promoted.ready_after(2), 1);
        assert_eq!(promoted.total(), 6);
    }
    #[test]
    fn chunk_tags_recover_each_planned_progress_position() {
        let units = reader_core::text::plan_units(
            &format!("{}. Second sentence.", "word ".repeat(150)),
            240,
        )
        .unwrap();
        assert!(units.len() > 2);
        for (index, unit) in units.iter().enumerate() {
            let (sentence, chunk) = super::tags(unit);
            let progress =
                reader_core::text::chunk_progress(&units, sentence as usize, chunk as usize)
                    .unwrap();
            assert_eq!(progress.index, index);
            assert_eq!(progress.total, units.len());
        }
    }
    #[test]
    fn first_chunk_is_available_before_producer_finishes_and_order_is_preserved() {
        let (tx, rx) = super::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let producer = std::thread::spawn(move || {
            assert!(tx.send((0, "first"), &|| true));
            gate.recv().unwrap();
            assert!(tx.send((1, "second"), &|| true));
        });
        assert_eq!(rx.receive(&|| true).unwrap(), Some((0, "first")));
        assert!(!producer.is_finished());
        release.send(()).unwrap();
        assert_eq!(rx.receive(&|| true).unwrap(), Some((1, "second")));
        assert_eq!(rx.receive(&|| true).unwrap(), None);
        producer.join().unwrap();
    }
    #[test]
    fn dropping_playback_wakes_a_full_producer() {
        let (tx, rx) = super::channel();
        assert!(tx.send(1, &|| true));
        let producer = std::thread::spawn(move || tx.send(2, &|| true));
        drop(rx);
        assert!(!producer.join().unwrap());
    }
}
