//! Stable XDG Desktop Portal identity for the resident host application.

const APP_ID: &str = "io.github.yosef0H4.simple_stt";

pub async fn register() -> anyhow::Result<()> {
    use anyhow::Context;

    super::ensure_host_desktop_entry()?;
    ashpd::register_host_app(APP_ID.parse()?)
        .await
        .context("registering Simple STT with the XDG portal host registry")
}

#[cfg(test)]
mod tests {
    use super::APP_ID;

    #[test]
    fn app_id_matches_packaged_portal_desktop_entry() {
        let desktop = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../packaging/linux/io.github.yosef0H4.simple_stt.desktop");
        let desktop_id = desktop
            .file_stem()
            .and_then(std::ffi::OsStr::to_str)
            .expect("desktop entry has a UTF-8 file name")
            .to_owned();
        let body = std::fs::read_to_string(desktop).expect("packaged portal desktop entry exists");

        assert_eq!(APP_ID, desktop_id);
        assert!(body.lines().any(|line| line == "Name=Simple STT"));
        assert!(body.lines().any(|line| line == "Icon=simple-stt"));
    }
}
