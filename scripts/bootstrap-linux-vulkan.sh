#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runtime="$root/external/parakeet-runtime/parakeet-linux-vulkan/bin"
models="$root/external/parakeet-runtime/models"
temporary="$(mktemp -d)"
trap 'rm -rf -- "$temporary"' EXIT

curl -fL 'https://github.com/mudler/parakeet.cpp/releases/download/v0.5.0/parakeet-v0.5.0-lib-linux-vulkan-x64.tar.gz' -o "$temporary/runtime.tar.gz"
echo 'a8e7a5af6ef82088a71b0e34708c31bf6ab9f69624bcd2cff2ad7e8c11e7e494  '"$temporary/runtime.tar.gz" | sha256sum -c -
tar -xzf "$temporary/runtime.tar.gz" -C "$temporary"
library="$(find "$temporary" -type f -name libparakeet.so -print -quit)"
if [[ -z "$library" ]]; then
  echo 'Official Vulkan archive did not contain libparakeet.so' >&2
  exit 1
fi
mkdir -p "$runtime" "$models"
cp -- "$library" "$runtime/libparakeet.so"

english="$models/tdt_ctc-110m-q8_0.gguf"
if [[ ! -f "$english" ]]; then
  curl -fL 'https://huggingface.co/mudler/parakeet-cpp-gguf/resolve/main/tdt_ctc-110m-q8_0.gguf' -o "$english.partial"
  mv -- "$english.partial" "$english"
fi
echo '614feee3a990cf0e672b0314f4da0c80ae8da9094507f5ccb7c42e43b5fc5a12  '"$english" | sha256sum -c -

arabic="$models/lemura-arabic-asr-lite-q8_0.gguf"
if [[ $# -gt 0 ]]; then
  echo 'b0aa3f0f316551a45bbd76d1ac7674102a3b221d5fd8c4cd6cac1c2bc4ccce86  '"$1" | sha256sum -c -
  cp -- "$1" "$arabic"
elif [[ ! -f "$arabic" ]]; then
  curl -fL 'https://huggingface.co/yosef0H4/lemura-arabic-asr-lite-GGUF/resolve/main/lemura-arabic-asr-lite-q8_0.gguf' -o "$arabic.partial"
  echo 'b0aa3f0f316551a45bbd76d1ac7674102a3b221d5fd8c4cd6cac1c2bc4ccce86  '"$arabic.partial" | sha256sum -c -
  mv -- "$arabic.partial" "$arabic"
fi
echo 'b0aa3f0f316551a45bbd76d1ac7674102a3b221d5fd8c4cd6cac1c2bc4ccce86  '"$arabic" | sha256sum -c -
echo "Vulkan runtime ready: $runtime"
