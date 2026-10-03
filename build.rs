use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

fn sha256(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn checked_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(format!("invalid frontend manifest path: {relative}"));
    }
    Ok(root.join(path))
}

fn collect_files(
    root: &Path,
    directory: &Path,
    files: &mut BTreeSet<String>,
) -> Result<(), String> {
    let entries =
        fs::read_dir(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            collect_files(root, &path, files)?;
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            files.insert(relative);
        }
    }
    Ok(())
}

fn verify_frontend(root: &Path) -> Result<(), String> {
    let frontend = root.join("web/settings");
    println!("cargo:rerun-if-changed={}", frontend.join("src").display());
    println!(
        "cargo:rerun-if-changed={}",
        frontend.join("tools").display()
    );
    let manifest_path = frontend.join("dist/manifest.json");
    println!("cargo:rerun-if-changed={}", manifest_path.display());
    let raw = fs::read_to_string(&manifest_path).map_err(|_| {
        "frontend build output is missing; run `cd web/settings && npm ci && npm run build`"
            .to_owned()
    })?;
    let manifest: Value = serde_json::from_str(&raw).map_err(|error| {
        format!(
            "invalid frontend manifest {}: {error}",
            manifest_path.display()
        )
    })?;
    for group in ["sources", "assets"] {
        if !manifest.get(group).is_some_and(Value::is_object) {
            return Err(format!(
                "frontend manifest must contain an object named `{group}`"
            ));
        }
    }

    let shared = manifest["shared_sources"]
        .as_object()
        .ok_or("frontend manifest needs shared_sources")?;
    let expected = BTreeSet::from([
        "assets/fonts/NotoSansArabic-Regular.ttf",
        "assets/fonts/NotoSansArabic-Regular.woff2",
        "assets/fonts/LICENSE-NotoSansArabic.txt",
    ]);
    if shared.keys().map(String::as_str).collect::<BTreeSet<_>>() != expected {
        return Err("font source manifest differs from expected inputs".into());
    }
    for (relative, expected) in shared {
        let path = checked_path(root, relative)?;
        println!("cargo:rerun-if-changed={}", path.display());
        if expected.as_str() != Some(&sha256(&path)?) {
            return Err(format!(
                "shared frontend source {relative} is stale; run npm run build"
            ));
        }
    }
    let mut digest = Sha256::new();
    for name in ["desktop.en.json", "desktop.ar.json"] {
        let path = frontend.join("src/lib/locales").join(name);
        let bytes = fs::read(&path).map_err(|e| e.to_string())?;
        let catalog: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if !catalog.is_object() {
            return Err("desktop catalogs must be objects".into());
        }
        digest.update(bytes);
    }
    let generated = root.join("ahk/lib/Locale.ahk");
    println!("cargo:rerun-if-changed={}", generated.display());
    let expected = format!("; catalog-sha256: {:x}", digest.finalize());
    if !fs::read_to_string(generated)
        .map_err(|e| e.to_string())?
        .contains(&expected)
    {
        return Err("AHK translations are stale; run node web/settings/tools/locales.mjs".into());
    }

    let sources = manifest["sources"]
        .as_object()
        .expect("validated sources object");
    let required_sources = [
        "index.html",
        "package.json",
        "package-lock.json",
        "vite.config.ts",
        "tsconfig.json",
        "svelte.config.js",
    ];
    for name in required_sources {
        if !sources.contains_key(name) {
            return Err(format!("frontend manifest omits required source `{name}`"));
        }
    }
    let mut expected_sources = BTreeSet::new();
    collect_files(&frontend, &frontend.join("src"), &mut expected_sources)?;
    collect_files(&frontend, &frontend.join("tools"), &mut expected_sources)?;
    expected_sources.extend(
        [
            "index.html",
            "package.json",
            "package-lock.json",
            "vite.config.ts",
            "tsconfig.json",
            "svelte.config.js",
        ]
        .into_iter()
        .map(str::to_owned),
    );
    let manifest_sources: BTreeSet<_> = sources.keys().cloned().collect();
    if manifest_sources != expected_sources {
        let missing: Vec<_> = expected_sources
            .difference(&manifest_sources)
            .cloned()
            .collect();
        let extra: Vec<_> = manifest_sources
            .difference(&expected_sources)
            .cloned()
            .collect();
        return Err(format!(
            "frontend manifest source list differs from current inputs (missing: {}; extra: {}); run `cd web/settings && npm run build`",
            missing.join(", "),
            extra.join(", ")
        ));
    }
    if !sources.keys().any(|path| path.starts_with("src/"))
        || !sources.keys().any(|path| path.starts_with("tools/"))
    {
        return Err("frontend manifest must fingerprint `src/` and `tools/` inputs".into());
    }
    for (relative, expected) in sources {
        let path = checked_path(&frontend, relative)?;
        println!("cargo:rerun-if-changed={}", path.display());
        let actual = sha256(&path)?;
        if expected.as_str() != Some(&actual) {
            return Err(format!(
                "frontend source `{relative}` changed after the last build; run `cd web/settings && npm run build`"
            ));
        }
    }

    let assets = manifest["assets"]
        .as_object()
        .expect("validated assets object");
    for name in ["index.html", "app.js", "styles.css", "arabic.woff2"] {
        if !assets.contains_key(name) {
            return Err(format!("frontend manifest omits required asset `{name}`"));
        }
    }
    if assets.len() != 4 {
        return Err(
            "frontend manifest assets must contain exactly index.html, app.js, styles.css, and arabic.woff2"
                .into(),
        );
    }
    for (relative, expected) in assets {
        let path = checked_path(&frontend.join("dist"), relative)?;
        println!("cargo:rerun-if-changed={}", path.display());
        let actual = sha256(&path)?;
        if expected.as_str() != Some(&actual) {
            return Err(format!(
                "frontend asset `dist/{relative}` is stale or modified; run `cd web/settings && npm run build`"
            ));
        }
    }
    Ok(())
}

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets manifest dir"));
    if let Err(error) = verify_frontend(&root) {
        panic!("frontend asset verification failed: {error}");
    }

    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-changed=resources/windows.rc");
        println!("cargo:rerun-if-changed=resources/simple-stt.exe.manifest");
        embed_resource::compile("resources/windows.rc", embed_resource::NONE)
            .manifest_required()
            .expect("compile Windows resources");
    }
}
