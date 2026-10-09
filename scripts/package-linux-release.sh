#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
choice="${1:-all}"
case "$choice" in
  fedora|arch|all) ;;
  *) echo "usage: $0 [fedora|arch|all]" >&2; exit 2 ;;
esac
out="$root/artifacts/linux-release"
mkdir -p "$out"
if [[ "$(uname -m)" != x86_64 ]]; then
  echo 'Linux packages are currently built for x86_64 only' >&2
  exit 1
fi
version="$(awk '/^\[package\]/{pkg=1; next} pkg && /^version = /{gsub(/"/, "", $3); print $3; exit}' "$root/Cargo.toml")"
snapshot="$out/.build-source"
source_archive_tmp="$out/.simple-stt-$version-source.tar.gz"
source_archive=""
source_hash_file="$out/.source-sha256"

cleanup() {
  python3 -c 'import shutil,sys; shutil.rmtree(sys.argv[1], ignore_errors=True)' "$snapshot"
}
trap cleanup EXIT

mkdir -p "$snapshot"
source_date_epoch="${SOURCE_DATE_EPOCH:-$(git -C "$root" log -1 --format=%ct 2>/dev/null || date +%s)}"
tar -C "$root" --sort=name --mtime="@$source_date_epoch" \
  --owner=0 --group=0 --numeric-owner \
  --exclude='./.git' --exclude='./target' --exclude='./external' \
  --exclude='./.codex' --exclude='./.agents' --exclude='./.vscode' --exclude='./.idea' \
  --exclude='__pycache__' --exclude='*.pyc' \
  --exclude='./artifacts' --exclude='./web/settings/node_modules' \
  --exclude='./resources/bin' -czf "$source_archive_tmp" .
tar -xzf "$source_archive_tmp" -C "$snapshot"
sha256sum "$source_archive_tmp" | awk '{print $1}' > "$source_hash_file"
source_archive="$out/simple-stt-$version-source-$(cut -c1-12 "$source_hash_file").tar.gz"
mv -f "$source_archive_tmp" "$source_archive"

if ! command -v podman >/dev/null 2>&1; then
  echo 'podman is required; install rootless Podman and retry' >&2
  exit 1
fi

build_one() {
  local distro="$1"
  local image="localhost/simple-stt-linux-builder-$distro"
  local source_volume="simple-stt-linux-$distro-work"
  printf 'Source archive: %s sha256=%s\n' \
    "$(basename "$source_archive")" "$(cat "$source_hash_file")" > "$out/BUILD-INFO-$distro.txt"
  printf 'Runtime: Parakeet v0.5.0 x86_64 Vulkan, archive sha256=a8e7a5af6ef82088a71b0e34708c31bf6ab9f69624bcd2cff2ad7e8c11e7e494\n' >> "$out/BUILD-INFO-$distro.txt"
  podman build --pull=always --memory=4g --cpu-period=100000 --cpu-quota=200000 --jobs=1 \
    -f "$root/packaging/linux/$distro/Containerfile" \
    -t "$image" "$root"
  podman volume inspect "$source_volume" >/dev/null 2>&1 || podman volume create "$source_volume" >/dev/null
  podman run --rm --memory=4g --cpus=2 --userns=keep-id:uid=1000,gid=1000 --user 1000:1000 \
    --security-opt label=disable \
    -v "$source_volume:/work:rw" \
    -v "$snapshot:/source:ro" -v "$out:/out:rw" \
    "$image"

  local package
  local smoke_command
  local package_mount
  if [[ "$distro" == fedora ]]; then
    package="$out/simple-stt-$version-$(awk '/^Release:/{print $2}' "$root/packaging/linux/fedora/simple-stt.spec" | sed 's/%{?dist}/.fc44/').x86_64.rpm"
    package_mount=/package.rpm
  else
    package="$out/simple-stt-$version-$(sed -n 's/^pkgrel=//p' "$root/packaging/linux/arch/PKGBUILD")-x86_64.pkg.tar.zst"
    package_mount=/package.pkg.tar.zst
  fi
  test -n "$package"
  local smoke_image
  if [[ "$distro" == fedora ]]; then
    smoke_image=quay.io/fedora/fedora:44
    smoke_command="dnf -y install python3 binutils shadow-utils util-linux '$package_mount' && useradd -m tester && runuser -u tester -- python3 /test-linux-package.py"
  else
    smoke_image=docker.io/library/archlinux:base
    smoke_command="pacman -Syu --noconfirm python binutils shadow util-linux && pacman -U --noconfirm '$package_mount' && useradd -m tester && runuser -u tester -- python3 /test-linux-package.py"
  fi
  podman run --rm --memory=4g --cpus=2 --user 0:0 \
    --security-opt label=disable \
    -v "$package:$package_mount:ro" \
    -v "$snapshot/scripts/test-linux-package.py:/test-linux-package.py:ro" \
    --entrypoint /usr/bin/bash "$smoke_image" -lc "$smoke_command"

  local image_id toolchain package_hash
  image_id="$(podman image inspect --format '{{.Id}}' "$image")"
  toolchain="$(podman run --rm --memory=4g --cpus=2 --userns=keep-id:uid=1000,gid=1000 --user 1000:1000 \
    --entrypoint rustc "$image" --version)"
  package_hash="$(sha256sum "$package" | cut -d' ' -f1)"
  local runtime_closure
  runtime_closure="$(tr '\n' ',' < "$out/parakeet-ldd-$distro.txt")"
  printf '%s\n' \
    "$distro image=$image" \
    "$distro image_id=$image_id" \
    "$distro toolchain=$toolchain" \
    "$distro package=$(basename "$package") sha256=$package_hash" \
    "$distro parakeet-runtime-closure=$runtime_closure" \
    "$distro smoke=PASS" >> "$out/BUILD-INFO-$distro.txt"
}

case "$choice" in
  fedora) build_one fedora 2>&1 | tee "$out/fedora-build.log" ;;
  arch) build_one arch 2>&1 | tee "$out/arch-build.log" ;;
  all) build_one fedora 2>&1 | tee "$out/fedora-build.log"; build_one arch 2>&1 | tee "$out/arch-build.log" ;;
  *) echo "usage: $0 [fedora|arch|all]" >&2; exit 2 ;;
esac

for distro in fedora arch; do
  if [[ -f "$out/BUILD-INFO-$distro.txt" ]]; then
    cat "$out/BUILD-INFO-$distro.txt"
  fi
done > "$out/BUILD-INFO.txt"

{
  find "$out" -maxdepth 1 -type f \( -name 'simple-stt-*.rpm' -o -name 'simple-stt-*.pkg.tar.zst' -o -name 'simple-stt-*-source-*.tar.gz' \) -printf '%f\n'
} | sort -u | while IFS= read -r filename; do
  (cd "$out" && sha256sum "$filename")
done > "$out/SHA256SUMS"

printf 'Linux packages are in %s\n' "$out"

if [[ -f "$out/BUILD-INFO-fedora.txt" && -f "$out/BUILD-INFO-arch.txt" ]]; then
  cat "$out/BUILD-INFO-fedora.txt" "$out/BUILD-INFO-arch.txt" > "$out/BUILD-INFO.txt"
else
  cp "$out/BUILD-INFO-$choice.txt" "$out/BUILD-INFO.txt"
fi
