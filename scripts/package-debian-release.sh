#!/usr/bin/env bash
# Build a native Debian package from this checkout, without installing it.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
[[ "$(dpkg --print-architecture)" == amd64 ]] || { echo 'Only amd64 is supported' >&2; exit 1; }
version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)"
stage="$(mktemp -d "${TMPDIR:-/tmp}/simple-stt-debian.XXXXXXXX")"
trap 'rm -rf "$stage"' EXIT
python3 scripts/verify-settings-frontend.py
python3 scripts/build-linux-fast-paste.py
test -x resources/bin/linux-fast-paste
cargo build --release --locked --jobs "${CARGO_BUILD_JOBS:-2}" --workspace --bins
app="$stage/usr/lib/simple-stt"
mkdir -p "$app" "$stage/usr/bin" "$stage/DEBIAN"
for binary in simple-stt-linux simple-stt-capture simple-stt-infer simple-stt-ctl simple-stt-settings simple-stt-settings-window readerd readerctl reader-tts reader-ocr reader-hud; do
  install -m755 "${CARGO_TARGET_DIR:-target}/release/$binary" "$app/$binary"
done
install -m755 resources/bin/linux-fast-paste "$app/"
install -m755 packaging/linux/simple-stt "$stage/usr/bin/"
ln -s ../lib/simple-stt/simple-stt-ctl "$stage/usr/bin/simple-stt-ctl"
for file in fixtures/parakeet-smoke.wav fixtures/asr/arabic-sa.wav; do
  install -Dm644 "$file" "$app/$file"
done
for file in downloads.json; do install -m644 "assets/catalog/$file" "$app/"; done
install -m644 assets/language/lid.176.ftz "$app/"
printf 'Installed runtime root; models use the per-user data directory.\n' > "$app/linux-installation"
for file in packaging/linux/*.desktop; do install -Dm644 "$file" "$stage/usr/share/applications/$(basename "$file")"; done
install -Dm644 packaging/linux/simple-stt.svg "$stage/usr/share/icons/hicolor/scalable/apps/simple-stt.svg"
install -Dm644 packaging/linux/simple-stt-linux.service "$stage/usr/lib/systemd/user/simple-stt-linux.service"
install -Dm644 packaging/linux/simple-stt-server.service "$stage/usr/lib/systemd/user/simple-stt-server.service"
install -Dm644 LICENSE "$stage/usr/share/doc/simple-stt/copyright"
install -m644 THIRD_PARTY_NOTICES.md assets/language/ATTRIBUTION.md assets/language/LICENSE-CC-BY-SA-3.0.txt assets/fonts/LICENSE-*.txt "$stage/usr/share/doc/simple-stt/"
install -m644 web/settings/public/settings-sans-license.txt "$stage/usr/share/doc/simple-stt/"
cat > "$stage/DEBIAN/control" <<EOF
Package: simple-stt
Version: $version
Architecture: amd64
Maintainer: Simple STT contributors
Depends: libasound2t64, libcairo2, libpango-1.0-0, libpangocairo-1.0-0, libpipewire-0.3-0, libsecret-tools, libvulkan1, ffmpeg, xdg-utils, wl-clipboard, xclip, slurp, libatspi2.0-0t64, libxtst6, libwebkit2gtk-4.1-0
Description: Local speech, reading and OCR with a headless API
EOF
mkdir -p artifacts/linux-release
dpkg-deb --root-owner-group --build "$stage" "artifacts/linux-release/simple-stt_${version}_amd64.deb"
