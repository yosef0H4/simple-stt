use simple_stt::capture::inference_supervisor::{
    nonzero_pid, shutdown_shared, shutdown_shared_if_current, WorkerConfig, WorkerSupervisor,
};
use simple_stt::config::{InferenceDevice, LogLevel};
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
        inference_device: InferenceDevice::Cpu,
        speech_language: "en".into(),
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
fn repeated_recordings_skip_priming_and_progress_until_worker_replacement() {
    let config = worker_config(
        "normal.gguf",
        Duration::from_millis(20),
        Duration::from_millis(300),
    );
    let mut worker = WorkerSupervisor::new(config.clone());
    let mut loading = 0;
    let mut loaded = 0;
    assert!(worker
        .warm_up_with_progress(|| loading += 1, || loaded += 1)
        .unwrap());
    let pid = worker.worker_pid();
    worker.transcribe_pcm(1, &[1, 2, 3]).unwrap();
    for _ in 0..3 {
        assert!(!worker
            .warm_up_with_progress(|| loading += 1, || loaded += 1)
            .unwrap());
        assert_eq!(worker.worker_pid(), pid);
    }
    assert_eq!((loading, loaded), (1, 1));
    // Regional/language changes with the same model keep its readiness too.
    let mut next = config;
    next.speech_language = "ar".into();
    worker.replace_config(next).unwrap();
    assert!(!worker
        .warm_up_with_progress(|| loading += 1, || loaded += 1)
        .unwrap());
    assert_eq!(worker.worker_pid(), pid);
    thread::sleep(Duration::from_millis(40));
    assert!(worker.shutdown_if_idle(false).unwrap());
    assert!(worker
        .warm_up_with_progress(|| loading += 1, || loaded += 1)
        .unwrap());
    assert_ne!(worker.worker_pid(), pid);
    assert_eq!((loading, loaded), (2, 2));
    worker.shutdown_now().unwrap();
}

#[test]
fn successful_transcription_already_primes_the_worker() {
    let mut worker = WorkerSupervisor::new(worker_config(
        "normal.gguf",
        Duration::from_secs(10),
        Duration::from_millis(300),
    ));
    worker.transcribe_pcm(1, &[1, 2, 3]).unwrap();
    let pid = worker.worker_pid();
    assert!(!worker
        .warm_up_with_progress(
            || panic!("unexpected loading"),
            || panic!("unexpected loaded")
        )
        .unwrap());
    worker
        .transcribe_wav(2, &PathBuf::from("fixture.wav"))
        .unwrap();
    assert!(!worker
        .warm_up_with_progress(
            || panic!("unexpected loading"),
            || panic!("unexpected loaded")
        )
        .unwrap());
    assert_eq!(worker.worker_pid(), pid);
    worker.shutdown_now().unwrap();
}

#[test]
fn obsolete_queued_shutdown_preserves_the_current_ready_worker() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let shared = Arc::new(Mutex::new(WorkerSupervisor::new(worker_config(
        "normal.gguf",
        Duration::from_secs(10),
        Duration::from_millis(300),
    ))));
    let mut held = shared.lock().unwrap();
    held.warm_up(|| {}).unwrap();
    let pid = held.worker_pid();
    let tracker = held.pid_tracker();
    let current = Arc::new(AtomicBool::new(true));
    let still_current = Arc::clone(&current);
    let pending_worker = Arc::clone(&shared);
    let pending = thread::spawn(move || {
        shutdown_shared_if_current(
            pending_worker,
            tracker,
            Duration::from_millis(300),
            move || still_current.load(Ordering::SeqCst),
        )
    });
    thread::sleep(Duration::from_millis(30));
    current.store(false, Ordering::SeqCst);
    drop(held);
    pending.join().unwrap().unwrap();
    let mut worker = shared.lock().unwrap();
    assert_eq!(worker.worker_pid(), pid);
    assert!(!worker
        .warm_up_with_progress(
            || panic!("unexpected loading"),
            || panic!("unexpected loaded")
        )
        .unwrap());
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
    assert!(worker.shutdown_if_idle(false).unwrap());
    assert_eq!(worker.worker_pid(), None);
}

#[test]
fn active_work_defers_worker_idle_timeout() {
    let mut worker = WorkerSupervisor::new(worker_config(
        "normal.gguf",
        Duration::from_millis(20),
        Duration::from_millis(300),
    ));
    worker.transcribe_pcm(1, &[1]).unwrap();
    let warm_pid = worker.worker_pid();
    thread::sleep(Duration::from_millis(60));
    assert!(!worker.shutdown_if_idle(true).unwrap());
    assert_eq!(worker.worker_pid(), warm_pid);
    thread::sleep(Duration::from_millis(10));
    assert!(!worker.shutdown_if_idle(false).unwrap());
    assert_eq!(worker.worker_pid(), warm_pid);
    thread::sleep(Duration::from_millis(30));
    assert!(worker.shutdown_if_idle(false).unwrap());
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
fn saving_assignments_does_not_replace_the_warm_selected_model() {
    let mut worker = WorkerSupervisor::new(worker_config(
        "normal.gguf",
        Duration::from_secs(10),
        Duration::from_millis(300),
    ));
    worker.transcribe_pcm(1, &[1]).unwrap();
    let pid = worker.worker_pid().unwrap();
    worker
        .update_runtime_config(worker_config(
            "another-saved-selection.gguf",
            Duration::from_secs(10),
            Duration::from_millis(300),
        ))
        .unwrap();
    assert_eq!(worker.worker_pid(), Some(pid));
    worker.transcribe_pcm(2, &[1]).unwrap();
    assert_eq!(worker.worker_pid(), Some(pid));
    worker.shutdown_now().unwrap();
}

#[test]
fn device_switch_recycles_worker_before_next_request() {
    let mut first = worker_config(
        "normal.gguf",
        Duration::from_secs(10),
        Duration::from_millis(300),
    );
    first.inference_device = InferenceDevice::Cpu;

    let mut worker = WorkerSupervisor::new(first);
    worker.transcribe_pcm(1, &[1]).unwrap();
    let first_pid = worker.worker_pid().unwrap();

    let mut second = worker_config(
        "normal.gguf",
        Duration::from_secs(10),
        Duration::from_millis(300),
    );
    second.inference_device = InferenceDevice::Gpu;

    worker.replace_config(second).unwrap();
    assert_eq!(worker.worker_pid(), None);

    worker.transcribe_pcm(2, &[2]).unwrap();
    assert_ne!(worker.worker_pid(), Some(first_pid));
    worker.shutdown_now().unwrap();
}

#[test]
fn language_switch_reuses_worker_when_model_path_is_unchanged() {
    let first = worker_config(
        "normal.gguf",
        Duration::from_secs(10),
        Duration::from_millis(300),
    );
    let mut worker = WorkerSupervisor::new(first.clone());
    worker.transcribe_pcm(1, &[1]).unwrap();
    let first_pid = worker.worker_pid().unwrap();
    let mut second = first;
    second.speech_language = "ar".into();
    worker.replace_config(second).unwrap();
    assert_eq!(worker.worker_pid(), Some(first_pid));
    worker.transcribe_pcm(2, &[2]).unwrap();
    assert_eq!(worker.worker_pid(), Some(first_pid));
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
