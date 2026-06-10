from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
path = ROOT / "src/bin/uvox_capture.rs"
text = path.read_text(encoding="utf-8")

old = '''        ShellCommand::ListModels => {
            let mut response = ShellResponse::ok("approved models");
            for (index, model) in uvox::models::catalog().into_iter().enumerate() {
                response.values.insert(
                    format!("model.{index:03}"),
                    format!("{}|{}|{}", model.file, model.size_mb, model.recommended),
                );
            }
            response
        }
'''
new = '''        ShellCommand::ListModels => {
            let mut response = ShellResponse::ok("cached models");
            for (index, model) in uvox::models::catalog_for_config(config).into_iter().enumerate() {
                response.values.insert(
                    format!("model.{index:03}"),
                    format!("{}|{}|{}", model.file, model.size_mb, model.recommended),
                );
            }
            response
        }
        ShellCommand::RefreshModels => match uvox::models::refresh_catalog_cache() {
            Ok(files) => {
                let mut response = ShellResponse::ok("model catalog refreshed");
                response.values.insert("count".into(), files.len().to_string());
                response
            }
            Err(error) => ShellResponse::error(error.to_string()),
        },
'''
print("patch helper ready")

count = text.count(old)
if count != 1:
    raise SystemExit(f"expected one ListModels block, found {count}")
path.write_text(text.replace(old, new, 1), encoding="utf-8")
print("patched capture refresh handler")
