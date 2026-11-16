# This PC as a speech and OCR server

This setup uses the installed Debian package, a systemd user service with boot
lingering, and token-authenticated LAN/Tailscale access. The API base URL is:

```text
http://100.85.156.101:8765/v1
```

## Generate your server token

Run these commands **on the server as your ordinary user**. They replace the
bootstrap token used during setup. Generating a token in the desktop keyring
alone does not replace the headless service's environment file.

```bash
simple-stt credentials generate server --activate
```

To retrieve it for your client, open `~/.config/simple-stt/server.env` locally and
copy only the value after `=`. Keep it private. Repeating the commands rotates
the token and requires updating every client.

`--activate` saves the generated key for the background service, restarts it,
and prints the activated key. `--stdout` only prints a fresh key: it does not
change the running server. Copying a printed-only key into a client gives HTTP
401 until that key is installed on the server.

## Connect another device

1. Install [Tailscale](https://tailscale.com/docs/how-to/quickstart) on your laptop
   or phone, and sign in to the same tailnet as this PC. Approve the device if
   your tailnet requires approval. See [adding a device](https://tailscale.com/docs/features/access-control/device-management/how-to/set-up).
2. Keep Tailscale connected on both devices. On a laptop, check the route with
   `tailscale ping 100.85.156.101`.
3. In your application's OpenAI-compatible provider settings, enter the base URL
   above and your server token as its API key.
4. Choose the feature-specific model below. These are API aliases for the
   server's selected models; downloading another model does not make it a
   separate per-request API model.

| Feature | Model | Endpoint |
| --- | --- | --- |
| Text to speech | `kokoro-v1` | `POST /v1/audio/speech` |
| Speech to text | `configured-stt` | `POST /v1/audio/transcriptions` |
| Image OCR | `simple-tts-ocr` | `POST /v1/chat/completions` or `/v1/responses` |

The OCR endpoint reads text from images. It does not provide general chat or
visual reasoning. This setup does not host a general-purpose chat LLM.

## Verify from the laptop

In Bash, read the token without saving it in shell history:

```bash
read -rsp 'Server token: ' SIMPLE_STT_TOKEN; echo
export SIMPLE_STT_TOKEN
curl --fail http://100.85.156.101:8765/v1/models \
  -H "Authorization: Bearer $SIMPLE_STT_TOKEN"
```

Generate a WAV:

```bash
curl --fail http://100.85.156.101:8765/v1/audio/speech \
  -H "Authorization: Bearer $SIMPLE_STT_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"model":"kokoro-v1","voice":"af_heart","input":"The server is ready.","response_format":"wav"}' \
  --output speech.wav
```

Transcribe a recording:

```bash
curl --fail http://100.85.156.101:8765/v1/audio/transcriptions \
  -H "Authorization: Bearer $SIMPLE_STT_TOKEN" \
  -F model=configured-stt -F language=en -F file=@recording.wav
```

## Use Simple STT on the laptop

Run these commands on the laptop to configure remote STT:

```bash
simple-stt-ctl config set speech.remote_openai_compatible \
  '{"base_url":"http://100.85.156.101:8765/v1","model":"configured-stt","fallback_to_local":false,"timeout_ms":180000}'
simple-stt-ctl config set speech.provider '"remote_openai_compatible"'
simple-stt-ctl credentials set stt --env SIMPLE_STT_TOKEN
```

For reading and OCR, set the laptop's reader providers and credentials:

```bash
simple-stt-ctl config --scope reader set speech.provider \
  '{"kind":"open_ai_compatible","base_url":"http://100.85.156.101:8765/v1","model":"kokoro-v1","voice":"af_heart","format":"wav"}'
simple-stt-ctl config --scope reader set ocr.provider \
  '{"kind":"open_ai_compatible","base_url":"http://100.85.156.101:8765/v1","model":"simple-tts-ocr","api":"chat_completions"}'
simple-stt-ctl credentials set speech --env SIMPLE_STT_TOKEN
simple-stt-ctl credentials set ocr --env SIMPLE_STT_TOKEN
unset SIMPLE_STT_TOKEN
```

Reload the laptop app afterward. On Windows, use `simple-stt-ctl.exe`; the same
JSON values apply, but shell quoting and environment-variable syntax differ.
Credentials stored this way require the client's OS credential vault.

## Manage the server

```bash
systemctl --user status simple-stt-server
journalctl --user -u simple-stt-server -n 50 --no-pager
systemctl --user restart simple-stt-server
systemctl --user stop simple-stt-server
systemctl --user disable simple-stt-server
simple-stt-ctl assets list
```

The service starts without a desktop login because lingering is enabled. Model
workers can unload when idle; the API stays running and reloads them on demand.
Keep the PC powered on and disable automatic suspend in its power settings.
No router port forwarding is needed for Tailscale. This host listens on
`0.0.0.0:8765`, accepting LAN and Tailscale connections with a bearer token.

On this PC, sleep, suspend, hibernate, and hybrid-sleep systemd targets are
masked so a sleep request cannot interrupt serving. To allow sleep again:

```bash
sudo systemctl unmask sleep.target suspend.target hibernate.target hybrid-sleep.target
```

## Installed setup

The Debian 13 host has 11 Q8 STT models (one for each of the ten pinned Parakeet
families plus Lemura Arabic), both Kokoro and Supertonic 3, and all 34 OCR
bundles. All 77 catalog assets, including runtime dependencies, are verified
installed. Models and assets occupy about 14 GB in the user data directory.

The active configuration uses Vulkan on the GTX 1070 for English and Arabic
STT, Kokoro on CPU for TTS, and the Arabic/English Paddle bundle on CPU for OCR.
CUDA libraries are installed, but the pinned CUDA 13 Parakeet runtime reports
`CUDA driver version is insufficient for CUDA runtime version` with this
machine's NVIDIA 550 driver. Vulkan inference was tested successfully. No GPU
driver upgrade was performed.

The API has been tested locally through the PC's Tailscale address and through
isolated loopback clients. Connecting a second physical device remains your
final network check. Tailscale access rules and device approval still apply.

Privacy settings verified on the live host: `diagnostics.log_transcripts=false`,
`privacy.log_content=false`, `privacy.hide_text_in_notifications=true`, and AI
cleanup disabled. Temporary audio/image files used for processing are removed
after requests; the reader's capture, requests, and CLI folders were empty after
the installed-client checks. Requests without a valid bearer token are rejected.

Validation on this host passed: inference with all 11 STT and 34 OCR bundles;
non-silent speech from both TTS models; the OpenAI SDK/client interoperability
suite against the installed binaries; installed-package smoke checks; formatting,
Clippy with warnings denied, static/Linux/IPC checks; and 354 Rust tests across
the workspace's release targets. Release tests were staged with the same
`lid.176.ftz` language asset shipped in the package. These checks do not establish
recognition accuracy for every supported language or native Windows behavior.

Two CLI issues were corrected during setup: STT GGUF IDs can now be installed
headlessly through `assets install`, and installations track the parent job
while dependencies download. Regression tests cover both paths.

Persistent setup files:

- Installed package: `/usr/lib/simple-stt/`; rebuild with
  `scripts/package-debian-release.sh`. The `.deb` remains under
  `artifacts/linux-release/`.
- Boot service source: `packaging/linux/simple-stt-server.service`; installed
  under `/usr/lib/systemd/user/` and enabled for this user.
- Token: `~/.config/simple-stt/server.env` (mode 0600).
- App configuration: `~/.config/simple-stt/instances/installed/config.json`.
- Reader configuration and assets:
  `~/.local/share/simple-stt/instances/installed/`.
- STT models: `~/.local/share/simple-stt/models/`.

Repository changes also include the CLI fix in `src/admin.rs`, its integration
test in `tests/admin_assets.rs`, the command example in `docs/cli.md`, and
installed-binary/runtime overrides in `scripts/test-openai-server-e2e.py`.

To select a different installed server model, use `assets select`, then restart
the service. Examples:

```bash
simple-stt-ctl assets select tts --model supertonic3 --runtime sherpa-cpu
simple-stt-ctl assets select ocr --model paddle-en-pp-ocrv5-mobile-rec --runtime paddle-cpu
simple-stt-ctl assets select stt --model tdt_ctc-110m-q8_0.gguf --runtime parakeet-vulkan
systemctl --user restart simple-stt-server
```

A 401 response means the token is missing or wrong. A connection failure means
the service, Tailscale route, tailnet access policy, or host firewall needs
checking. An inference error needs the service journal; model installation and
successful inference are separate checks.

New server configurations default to `0.0.0.0:8765`; the server remains disabled
until explicitly enabled. Existing saved addresses are preserved.
