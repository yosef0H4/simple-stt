use crossbeam_channel::{unbounded, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum TypingCommand {
    Queue {
        session_id: u64,
        target_window: isize,
        text: String,
    },
    Shutdown,
}

pub trait TextSink: Send + Sync + 'static {
    fn focused_window(&self) -> isize;
    fn send_text(&self, text: &str) -> anyhow::Result<()>;
}

pub struct Typist {
    tx: Sender<TypingCommand>,
    active_session: Arc<Mutex<Option<u64>>>,
}

impl Typist {
    pub fn spawn(sink: Arc<dyn TextSink>, chunk_chars: usize, interval: Duration) -> Self {
        let (tx, rx) = unbounded::<TypingCommand>();
        let active_session = Arc::new(Mutex::new(None::<u64>));
        let active_for_thread = Arc::clone(&active_session);
        thread::spawn(move || {
            while let Ok(command) = rx.recv() {
                match command {
                    TypingCommand::Queue {
                        session_id,
                        target_window,
                        text,
                    } => {
                        let chars: Vec<char> = text.chars().collect();
                        for chunk in chars.chunks(chunk_chars.max(1)) {
                            if *active_for_thread.lock().unwrap() != Some(session_id) {
                                break;
                            }
                            if sink.focused_window() != target_window {
                                *active_for_thread.lock().unwrap() = None;
                                break;
                            }
                            let value: String = chunk.iter().collect();
                            if let Err(error) = sink.send_text(&value) {
                                tracing::warn!(%error, "failed to send transcript chunk");
                                *active_for_thread.lock().unwrap() = None;
                                break;
                            }
                            thread::sleep(interval);
                        }
                    }
                    TypingCommand::Shutdown => break,
                }
            }
        });
        Self { tx, active_session }
    }

    pub fn begin_session(&self, session_id: u64) {
        *self.active_session.lock().unwrap() = Some(session_id);
    }

    pub fn queue(&self, session_id: u64, target_window: isize, text: String) {
        let _ = self.tx.send(TypingCommand::Queue {
            session_id,
            target_window,
            text,
        });
    }

    pub fn cancel(&self, session_id: u64) {
        let mut active = self.active_session.lock().unwrap();
        if *active == Some(session_id) {
            *active = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicIsize, Ordering};

    struct FakeSink {
        focused: AtomicIsize,
        sent: Mutex<String>,
    }

    impl TextSink for FakeSink {
        fn focused_window(&self) -> isize {
            self.focused.load(Ordering::SeqCst)
        }
        fn send_text(&self, text: &str) -> anyhow::Result<()> {
            self.sent.lock().unwrap().push_str(text);
            Ok(())
        }
    }

    #[test]
    fn typist_sends_when_focus_matches() {
        let sink = Arc::new(FakeSink {
            focused: AtomicIsize::new(9),
            sent: Mutex::new(String::new()),
        });
        let typist = Typist::spawn(sink.clone(), 2, Duration::from_millis(0));
        typist.begin_session(1);
        typist.queue(1, 9, "hello".to_owned());
        thread::sleep(Duration::from_millis(30));
        assert_eq!(&*sink.sent.lock().unwrap(), "hello");
    }

    #[test]
    fn typist_rejects_focus_change() {
        let sink = Arc::new(FakeSink {
            focused: AtomicIsize::new(8),
            sent: Mutex::new(String::new()),
        });
        let typist = Typist::spawn(sink.clone(), 2, Duration::from_millis(0));
        typist.begin_session(1);
        typist.queue(1, 9, "hello".to_owned());
        thread::sleep(Duration::from_millis(30));
        assert_eq!(&*sink.sent.lock().unwrap(), "");
    }

    #[test]
    fn cancel_interrupts_a_queue() {
        let sink = Arc::new(FakeSink {
            focused: AtomicIsize::new(9),
            sent: Mutex::new(String::new()),
        });
        let typist = Typist::spawn(sink.clone(), 1, Duration::from_millis(10));
        typist.begin_session(1);
        typist.queue(1, 9, "abcdefghij".to_owned());
        thread::sleep(Duration::from_millis(22));
        typist.cancel(1);
        thread::sleep(Duration::from_millis(60));
        assert!(sink.sent.lock().unwrap().len() < 10);
    }
}
