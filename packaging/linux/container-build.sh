#!/usr/bin/env bash
set -euo pipefail

distro="${1:?expected fedora or arch}"
source_dir=/source
work=/work/build
python3 -c 'import shutil; [shutil.rmtree("/work/build/" + name, ignore_errors=True) for name in ("src", "out", "stage", "rpmbuild", "arch-package")]'
mkdir -p "$work/src" "$work/out"
tar -C "$source_dir" -cf - . | tar -C "$work/src" -xf -
cd "$work/src"
python3 scripts/prepare-linux-package-source.py "$work/src"

npm --prefix web/settings ci
npm --prefix web/settings run build
python3 scripts/verify-settings-frontend.py
for feature in x11 xtst gio-2.0 atspi-2; do
  pkg-config --exists "$feature" || { echo "missing required helper feature: $feature" >&2; exit 1; }
done
python3 scripts/build-linux-fast-paste.py
test -x resources/bin/linux-fast-paste
nm -D resources/bin/linux-fast-paste > "$work/linux-fast-paste.symbols"
grep -q atspi_ "$work/linux-fast-paste.symbols"
grep -q g_dbus_ "$work/linux-fast-paste.symbols"
! grep -q XTestFakeKeyEvent "$work/linux-fast-paste.symbols"
cargo +1.99.0 build --release --locked --jobs "${CARGO_BUILD_JOBS:-2}" \
  --bin simple-stt-linux --bin simple-stt-capture --bin simple-stt-infer \
  --bin simple-stt-ctl --bin simple-stt-settings --bin reader-hud
cargo +1.99.0 build --release --locked --jobs "${CARGO_BUILD_JOBS:-2}" \
  -p readerd -p readerctl -p reader-tts -p reader-ocr

stage="$work/stage"
app="$stage/usr/lib/simple-stt"
mkdir -p "$stage/usr/bin" "$app" \
  "$stage/usr/share/applications" \
  "$stage/usr/share/icons/hicolor/scalable/apps" \
  "$stage/usr/lib/systemd/user" "$stage/usr/share/licenses/simple-stt"
install -m 0755 packaging/linux/simple-stt "$stage/usr/bin/simple-stt"
for binary in simple-stt-linux simple-stt-capture simple-stt-infer simple-stt-ctl simple-stt-settings readerd readerctl reader-tts reader-ocr reader-hud; do
  install -m 0755 "$CARGO_TARGET_DIR/release/$binary" "$app/$binary"
done
install -m 0755 resources/bin/linux-fast-paste "$app/linux-fast-paste"
install -D -m 0644 fixtures/parakeet-smoke.wav "$app/fixtures/parakeet-smoke.wav"
install -D -m 0644 fixtures/asr/arabic-sa.wav "$app/fixtures/asr/arabic-sa.wav"
install -m 0644 packaging/linux/io.github.yosef0H4.simple_stt.desktop "$stage/usr/share/applications/"
install -m 0644 packaging/linux/simple-stt-settings.desktop "$stage/usr/share/applications/"
install -m 0644 packaging/linux/simple-stt-readerd.desktop "$stage/usr/share/applications/"
install -m 0644 packaging/linux/simple-stt.svg "$stage/usr/share/icons/hicolor/scalable/apps/"
install -m 0644 packaging/linux/simple-stt-linux.service "$stage/usr/lib/systemd/user/"
install -m 0644 LICENSE "$stage/usr/share/licenses/simple-stt/"
install -m 0644 THIRD_PARTY_NOTICES.md "$stage/usr/share/licenses/simple-stt/"
install -m 0644 assets/catalog/downloads.json "$app/downloads.json"
install -m 0644 assets/language/lid.176.ftz "$app/lid.176.ftz"
install -m 0644 assets/language/LICENSE-CC-BY-SA-3.0.txt "$stage/usr/share/licenses/simple-stt/"
install -m 0644 assets/language/ATTRIBUTION.md "$stage/usr/share/licenses/simple-stt/fastText-ATTRIBUTION.md"
install -m 0644 assets/fonts/LICENSE-JetBrainsMono.txt "$stage/usr/share/licenses/simple-stt/OFL-JetBrainsMono.txt"
install -m 0644 assets/fonts/LICENSE-NotoSansArabic.txt "$stage/usr/share/licenses/simple-stt/OFL-NotoSansArabic.txt"
printf 'Installed runtime root; models use the per-user data directory.\n' > "$app/linux-installation"
if [[ "$distro" == fedora ]]; then
  topdir="$work/rpmbuild"
  mkdir -p "$topdir/SOURCES" "$topdir/SPECS" "$work/out"
  cp -a "$stage" "$topdir/SOURCES/stage"
  cp packaging/linux/fedora/simple-stt.spec "$topdir/SPECS/"
  rpmbuild -bb --define "_topdir $topdir" "$topdir/SPECS/simple-stt.spec"
  find "$topdir/RPMS" -type f -name '*.rpm' -exec cp -v '{}' "$work/out/" \;
elif [[ "$distro" == arch ]]; then
  pkgdir="$work/arch-package"
  mkdir -p "$pkgdir"
  cp packaging/linux/arch/PKGBUILD "$pkgdir/"
  tar -C "$work" -cf "$pkgdir/simple-stt-stage.tar" stage
  stage_hash="$(sha256sum "$pkgdir/simple-stt-stage.tar" | cut -d' ' -f1)"
  sed -i "s/sha256sums=('SKIP')/sha256sums=('${stage_hash}')/" "$pkgdir/PKGBUILD"
  (cd "$pkgdir" && makepkg --noconfirm --cleanbuild --force --nodeps)
  find "$pkgdir" -maxdepth 1 -type f -name '*.pkg.tar.zst' -exec cp -v '{}' "$work/out/" \;
else
  echo "unknown distro: $distro" >&2
  exit 2
fi

cp -a "$work/out/." /out/
printf 'Package contents built for %s\n' "$distro"
