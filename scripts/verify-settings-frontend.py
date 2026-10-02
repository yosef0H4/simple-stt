#!/usr/bin/env python3
"""Check the built Settings frontend manifest, freshness, and size budgets."""
from gzip import compress
import hashlib
import json
from pathlib import Path, PurePosixPath
import sys

ROOT = Path(__file__).resolve().parents[1]
WEB = ROOT / "web/settings"
DIST = WEB / "dist"
MANIFEST = DIST / "manifest.json"
ERRORS: list[str] = []


def resolve(base: Path, relative: str) -> Path | None:
    path = PurePosixPath(relative)
    if path.is_absolute() or not path.parts or any(part in (".", "..") for part in path.parts):
        ERRORS.append(f"invalid manifest path: {relative}")
        return None
    return base.joinpath(*path.parts)


try:
    data = json.loads(MANIFEST.read_text(encoding="utf-8"))
except (OSError, json.JSONDecodeError) as error:
    print(f"FAIL: frontend manifest unavailable: {error}")
    print("Build it with: cd web/settings && npm ci && npm run build")
    sys.exit(1)

for group in ("sources", "assets"):
    if not isinstance(data.get(group), dict):
        ERRORS.append(f"manifest must contain an object named {group!r}")

sources = data.get("sources", {})
assets = data.get("assets", {})
for name in ("index.html", "package.json", "package-lock.json", "vite.config.ts", "tsconfig.json", "svelte.config.js"):
    if name not in sources:
        ERRORS.append(f"manifest omits required source {name!r}")
if not any(path.startswith("src/") for path in sources):
    ERRORS.append("manifest must fingerprint frontend src/ files")
if not any(path.startswith("tools/") for path in sources):
    ERRORS.append("manifest must fingerprint frontend tools/ files")

expected_sources = {
    path.relative_to(WEB).as_posix()
    for directory in (WEB / "src", WEB / "tools")
    for path in directory.rglob("*")
    if path.is_file()
}
expected_sources.update((
    "index.html", "package.json", "package-lock.json", "vite.config.ts",
    "tsconfig.json", "svelte.config.js",
))
if set(sources) != expected_sources:
    missing = sorted(expected_sources - set(sources))
    extra = sorted(set(sources) - expected_sources)
    ERRORS.append(
        "manifest source list differs from current inputs "
        f"(missing: {', '.join(missing)}; extra: {', '.join(extra)})"
    )

for relative, expected in sources.items():
    path = resolve(WEB, relative)
    if path is None:
        continue
    try:
        actual = hashlib.sha256(path.read_bytes()).hexdigest()
    except OSError as error:
        ERRORS.append(f"source {relative!r} is missing: {error}")
        continue
    if actual != expected:
        ERRORS.append(f"source {relative!r} changed since the last frontend build")

for name in ("index.html", "app.js", "styles.css"):
    if name not in assets:
        ERRORS.append(f"manifest omits required asset {name!r}")

if set(assets) != {"index.html", "app.js", "styles.css"}:
    ERRORS.append("manifest assets must contain exactly index.html, app.js, and styles.css")

asset_bytes = 0
gzip_bytes = 0
for relative, expected in assets.items():
    path = resolve(DIST, relative)
    if path is None:
        continue
    try:
        content = path.read_bytes()
    except OSError as error:
        ERRORS.append(f"asset {relative!r} is missing: {error}")
        continue
    if hashlib.sha256(content).hexdigest() != expected:
        ERRORS.append(f"asset {relative!r} does not match its manifest hash")
    asset_bytes += len(content)
    if relative in ("app.js", "styles.css"):
        gzip_bytes += len(compress(content, mtime=0))

if asset_bytes > 300 * 1024:
    ERRORS.append(f"frontend assets total {asset_bytes} bytes; budget is 307200 bytes")
if gzip_bytes > 100 * 1024:
    ERRORS.append(f"app.js + styles.css gzip to {gzip_bytes} bytes; budget is 102400 bytes")

if ERRORS:
    print("FAIL: Settings frontend verification")
    for error in ERRORS:
        print(f" - {error}")
    print("Build it with: cd web/settings && npm run build")
    sys.exit(1)

print(f"PASS: Settings frontend hashes and budgets ({asset_bytes} bytes total, {gzip_bytes} gzip bytes for JS/CSS)")
