use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
        let manifest = root
            .join("../../../resources/simple-stt.exe.manifest")
            .canonicalize()
            .expect("Windows manifest");
        println!("cargo:rerun-if-changed={}", manifest.display());
        let resource = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("reader.rc");
        let manifest_path = manifest.to_string_lossy();
        let manifest_path = manifest_path
            .strip_prefix(r"\\?\")
            .unwrap_or(&manifest_path);
        std::fs::write(
            &resource,
            format!(
                "#pragma code_page(65001)\n1 24 \"{}\"\n",
                manifest_path.replace('\\', "/")
            ),
        )
        .expect("write reader resource");
        embed_resource::compile(&resource, embed_resource::NONE)
            .manifest_required()
            .expect("compile reader manifest");
    }
    println!("cargo:rustc-check-cfg=cfg(atspi_helper)");
    println!("cargo:rerun-if-changed=resources/active-app-atspi.c");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set"));
    let helper = out_dir.join("simple-tts-active-app");
    let flags = Command::new("pkg-config")
        .args(["--cflags", "--libs", "atspi-2", "gobject-2.0"])
        .output();
    let Ok(flags) = flags else { return };
    if !flags.status.success() {
        return;
    }
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir is set"));
    let mut command = Command::new(env::var("CC").unwrap_or_else(|_| "cc".into()));
    command
        .current_dir(&manifest)
        .arg("resources/active-app-atspi.c")
        .args(["-Wall", "-Wextra", "-Werror"])
        .arg("-o")
        .arg(&helper);
    for flag in String::from_utf8_lossy(&flags.stdout).split_whitespace() {
        command.arg(flag);
    }
    match command.status() {
        Ok(status) if status.success() => {
            println!(
                "cargo:rustc-env=SIMPLE_TTS_ACTIVE_APP_HELPER={}",
                helper.display()
            );
            println!("cargo:rustc-cfg=atspi_helper");
        }
        Ok(status) => panic!("AT-SPI helper compile failed (exit {status})"),
        Err(error) => panic!("could not run C compiler for AT-SPI helper: {error}"),
    }
}
