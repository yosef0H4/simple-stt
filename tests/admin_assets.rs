use simple_stt::config::AppConfig;
use std::process::Command;

#[test]
fn headless_asset_install_accepts_an_existing_stt_model_without_capture() {
    let temporary = tempfile::tempdir().unwrap();
    let model = "tdt_ctc-110m-q8_0.gguf";
    let models = temporary.path().join("models");
    std::fs::create_dir(&models).unwrap();
    // No inference occurs: installation must preserve an already present model.
    std::fs::write(models.join(model), b"existing model").unwrap();
    let mut config = AppConfig::default();
    config.speech.model_dir = models.to_string_lossy().into_owned();
    let path = temporary.path().join("config.json");
    std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_simple-stt-ctl"))
        .args(["assets", "install", model])
        .env("SIMPLE_STT_CONFIG", path)
        .env("XDG_DATA_HOME", temporary.path().join("data"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["installed"], model);
    assert_eq!(
        std::fs::read(models.join(model)).unwrap(),
        b"existing model"
    );
}
