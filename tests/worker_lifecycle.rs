use simple_stt::capture::inference_supervisor::{
    nonzero_pid, shutdown_shared, WorkerConfig, WorkerSupervisor,
};
use simple_stt::config::LogLevel;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

fn mock_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_simple_stt_mock_infer"))
}

fn worker_config(model_name: &str, idle: Duration, grace: Duration) -> WorkerConfig {
    let root = std::env::temp_dir().join(format!("simple-stt-worker-tests-{}", std::process::id()));
    WorkerConfig {
        executable: mock_binary(),
        runtime_dir: root.join("runtime"),
        model_path: root.join(model_name),
        log_path: root.join("simple-stt-mock-infer.log"),
        log_level: LogLevel::Debug,
        idle_timeout: idle,
        shutdown_grace: grace,
    }
}

#[test]
fn worker_launches_lazily_and_reuses_warm_process() {
    let mut worker = WorkerSupervisor::new(worker_config(
        "normal.gguf",
        Duration::from_secs(10),
        Duration::from_millis(300),
    ));
    assert_eq!(worker.worker_pid(), None);
    assert_eq!(
        worker.transcribe_pcm(1, &[1, 2, 3]).unwrap(),
        "mock مرحبا 世界 🙂"
    );
    let warm_pid = worker.worker_pid().unwrap();
    assert_eq!(
        worker.transcribe_pcm(2, &[4, 5, 6]).unwrap(),
        "mock مرحبا 世界 🙂"
    );
    assert_eq!(worker.worker_pid(), Some(warm_pid));
    worker.shutdown_now().unwrap();
    assert_eq!(worker.worker_pid(), None);
}

#[test]
fn warm_up_loads_and_primes_worker_before_first_transcript() {
    let mut worker = WorkerSupervisor::new(worker_config(
        "normal.gguf",
        Duration::from_secs(10),
        Duration::from_millis(300),
    ));
    let mut model_loaded = false;
    worker.warm_up(|| model_loaded = true).unwrap();
    assert!(model_loaded);
    let warm_pid = worker.worker_pid().unwrap();
    assert_eq!(
        worker.transcribe_pcm(1, &[1, 2, 3]).unwrap(),
        "mock مرحبا 世界 🙂"
    );
    assert_eq!(worker.worker_pid(), Some(warm_pid));
    worker.shutdown_now().unwrap();
}

#[test]
fn worker_exits_after_idle_timeout() {
    let mut worker = WorkerSupervisor::new(worker_config(
        "normal.gguf",
        Duration::from_millis(20),
        Duration::from_millis(300),
    ));
    worker.transcribe_pcm(1, &[1]).unwrap();
    thread::sleep(Duration::from_millis(60));
    assert!(worker.shutdown_if_idle().unwrap());
    assert_eq!(worker.worker_pid(), None);
}

#[test]
fn model_switch_recycles_worker_before_next_request() {
    let mut worker = WorkerSupervisor::new(worker_config(
        "first.gguf",
        Duration::from_secs(10),
        Duration::from_millis(300),
    ));
    worker.transcribe_pcm(1, &[1]).unwrap();
    assert!(worker.worker_pid().is_some());
    worker
        .replace_config(worker_config(
            "second.gguf",
            Duration::from_secs(10),
            Duration::from_millis(300),
        ))
        .unwrap();
    assert_eq!(worker.worker_pid(), None);
    assert_eq!(
        worker.transcribe_pcm(2, &[2]).unwrap(),
        "mock مرحبا 世界 🙂"
    );
    worker.shutdown_now().unwrap();
}

#[test]
fn crashed_worker_is_discarded_and_recoverable() {
    let mut worker = WorkerSupervisor::new(worker_config(
        "crash.gguf",
        Duration::from_secs(10),
        Duration::from_millis(300),
    ));
    assert!(worker.transcribe_pcm(1, &[1]).is_err());
    assert_eq!(worker.worker_pid(), None);
    worker
        .replace_config(worker_config(
            "normal.gguf",
            Duration::from_secs(10),
            Duration::from_millis(300),
        ))
        .unwrap();
    assert_eq!(
        worker.transcribe_pcm(2, &[2]).unwrap(),
        "mock مرحبا 世界 🙂"
    );
    worker.shutdown_now().unwrap();
}

#[test]
fn blocked_inference_is_force_terminated_by_exact_pid() {
    let shared = Arc::new(Mutex::new(WorkerSupervisor::new(worker_config(
        "hang.gguf",
        Duration::from_secs(10),
        Duration::from_millis(100),
    ))));
    let tracker = shared.lock().unwrap().pid_tracker();
    let request_worker = Arc::clone(&shared);
    let request = thread::spawn(move || request_worker.lock().unwrap().transcribe_pcm(1, &[1]));
    let deadline = Instant::now() + Duration::from_secs(3);
    while nonzero_pid(&tracker).is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        nonzero_pid(&tracker).is_some(),
        "mock worker did not launch"
    );
    thread::sleep(Duration::from_millis(50));
    let started = Instant::now();
    shutdown_shared(
        Arc::clone(&shared),
        Arc::clone(&tracker),
        Duration::from_millis(100),
    )
    .unwrap();
    assert!(started.elapsed() < Duration::from_secs(4));
    let _ = request.join().unwrap();
    assert_eq!(nonzero_pid(&tracker), None);
}
