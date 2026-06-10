from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
path = ROOT / "src/models.rs"
text = path.read_text(encoding="utf-8")

text = text.replace(
    "use std::collections::BTreeMap;\nuse std::collections::BTreeMap;",
    "use std::collections::BTreeMap;",
    1,
)
text = text.replace(
    'pub const CATALOG_URL: &str = "https://huggingface.co/api/models/mudler/parakeet-cpp-gguf/tree/main?recursive=false&expand=false";\n'
    'pub const CATALOG_URL: &str = "https://huggingface.co/api/models/mudler/parakeet-cpp-gguf/tree/main?recursive=false&expand=false";',
    'pub const CATALOG_URL: &str = "https://huggingface.co/api/models/mudler/parakeet-cpp-gguf/tree/main?recursive=false&expand=false";',
    1,
)

first = text.find("\nfn catalog_cache_path()")
second = text.find("\nfn catalog_cache_path()", first + 1)
end = text.find("\npub fn download_model", second)
assert first >= 0 and second >= 0 and end >= 0, (first, second, end)
text = text[:second] + text[end:]
path.write_text(text, encoding="utf-8")
print("deduped src/models.rs")
