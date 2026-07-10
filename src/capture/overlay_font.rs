//! Ships the visualizer font with the app and registers it with fontconfig at
//! runtime, so the overlay's block glyphs (▁▂▃…▇) render correctly even on
//! systems where the font is not installed.
//!
//! A missing font family silently falls back to a proportional font on Linux,
//! which renders the block glyphs with uneven widths/baselines. Bundling our
//! own copy removes that dependency.

use std::ffi::{c_void, CString};
use std::path::PathBuf;
use std::sync::OnceLock;

/// Bundled font: JetBrains Mono (SIL OFL 1.1). See assets/fonts/LICENSE-JetBrainsMono.txt.
const FONT_BYTES: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf");
const BUNDLED_FAMILY: &str = "JetBrains Mono";

static REGISTERED: OnceLock<bool> = OnceLock::new();

/// Ensure the bundled font is registered with fontconfig (idempotent). Must be
/// called before the first Pango layout is created so the font map picks it up.
pub fn ensure_registered() -> bool {
    *REGISTERED.get_or_init(|| match register() {
        Ok(()) => {
            tracing::debug!("overlay: registered bundled font '{BUNDLED_FAMILY}'");
            true
        }
        Err(error) => {
            tracing::warn!(
                "overlay: could not register bundled font ({error}); using system fonts"
            );
            false
        }
    })
}

fn cache_path() -> anyhow::Result<PathBuf> {
    let dir = dirs::cache_dir()
        .ok_or_else(|| anyhow::anyhow!("no cache dir"))?
        .join("simple-stt")
        .join("fonts");
    Ok(dir.join("JetBrainsMono-Regular.ttf"))
}

fn register() -> anyhow::Result<()> {
    let path = cache_path()?;
    let needs_write = match std::fs::metadata(&path) {
        Ok(meta) => meta.len() != FONT_BYTES.len() as u64,
        Err(_) => true,
    };
    if needs_write {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, FONT_BYTES)?;
    }

    // FcBool FcConfigAppFontAddFile(FcConfig *config, const FcChar8 *file)
    // config = NULL means the current configuration. fontconfig is already
    // loaded into the process by pango, so we dlopen it rather than link it.
    unsafe {
        let lib = libloading::Library::new("libfontconfig.so.1")
            .or_else(|_| libloading::Library::new("libfontconfig.so"))?;
        let add: libloading::Symbol<unsafe extern "C" fn(*const c_void, *const u8) -> i32> =
            lib.get(b"FcConfigAppFontAddFile\0")?;
        let cpath = CString::new(path.to_string_lossy().as_bytes())?;
        let ok = add(std::ptr::null(), cpath.as_ptr() as *const u8);
        // Keep the library mapped for the process lifetime.
        std::mem::forget(lib);
        if ok == 0 {
            anyhow::bail!(
                "FcConfigAppFontAddFile returned false for {}",
                path.display()
            );
        }
    }
    Ok(())
}
