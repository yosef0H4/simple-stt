# Media CLI

On Linux, `simple-stt-linux` exposes non-interactive media operations under
`simple-stt-linux media`. They use the same saved model, voice, device, and provider
configuration as the desktop services. These commands do not modify settings.

```text
simple-stt-linux media transcribe recording.mp4 [--language english|arabic] [--json]
simple-stt-ctl media transcribe recording.mp4 [--language english|arabic] [--json]
simple-stt-linux media speak "Text to read" [--output speech.wav]
simple-stt-linux media speak --file text.txt [--output speech.wav]
cat text.txt | simple-stt-linux media speak [--output speech.wav]
simple-stt-linux media ocr scan.png [--json]
```

Transcription accepts audio or video understood by `ffmpeg`. The CLI converts
the audio stream to a temporary 16 kHz mono PCM WAV, then sends sequential
chunks to the existing inference service. Chunks target 25 seconds, choosing
the quietest 100 ms window in a nearby search range as their boundary; no
samples are overlapped or discarded. Each chunk is capped at 35 seconds. If a
recording has no quiet pause near a boundary, a word can still be split across
chunks. The CLI joins chunk transcripts with spaces and does not guess at
duplicate words, since there is no overlapping audio to compare. `ffmpeg` must
be installed and available on `PATH`. Conversion is limited to 30 minutes and
512 MiB of decoded WAV data; only one bounded chunk is copied into memory at a
time. The temporary directory is removed after the command finishes.
`--language` overrides saved language routing for this file and accepts only
the app's English and Arabic speech languages.

Transcription and OCR write plain text to stdout by default. With `--json`,
stdout contains `{"text":"..."}`. Errors are reported on stderr with a failing
exit status. Speak accepts positional text, `--file`, or stdin. With no
explicit source, piped stdin is read automatically; an interactive terminal
with no text source exits with an error instead of waiting. `--stdin` forces
stdin input. Input is UTF-8 and limited to 1 MiB, with file and stdin reads
bounded before allocating more. Without `--output`, speech uses the installed
reader playback service. With `--output`, the service writes a WAV file. Text
is passed as data to the service API, never interpolated into a shell command.

When automatic language selection is enabled, file speech rendering routes
each detected text unit through its saved language assignment. Ambiguous units
inherit the current route. Output from different voices is combined in order
as 24 kHz mono PCM, with a five-minute audio limit. The running reader service uses the selected runtime and device for each route; the HTTP speech endpoints use supervised workers and the same saved assignments. Automatic detection uses the bundled compressed fastText language-identification model.

OCR accepts an image path and uses the currently configured OCR provider and
language. All paths are ordinary arguments, so spaces and shell metacharacters
in file names or text remain data.

## Headless administration

Both `simple-stt-ctl` and `simple-stt-linux` expose the same administration
commands. They read and validate the full app or reader JSON configuration
without starting audio or inference:

```text
simple-stt-ctl config [--scope app|reader] path|show|defaults
simple-stt-ctl config [--scope app|reader] get speech.language
simple-stt-ctl config [--scope app|reader] set server.enabled true
simple-stt-ctl config [--scope app|reader] import settings.json
simple-stt-ctl config [--scope app|reader] edit
```

`set` values must be JSON (`true`, `42`, `null`, objects, arrays, or quoted JSON
strings). Dot-separated keys can add map entries, and JSON Pointer paths such as
`/speech/language_models/ar` are accepted. `import` replaces the complete
configuration after validation. Reader writes retain revision checks. `edit`
uses `VISUAL` or `EDITOR` and validates the file after the editor exits.

`assets list` prints platform-available asset IDs with their dependency lists,
verified-install state, and model/runtime IDs. Use those IDs directly to install,
remove, or select assets. Selection always assigns a model and runtime
explicitly; installation does not change settings:

```text
simple-stt-ctl assets list
simple-stt-ctl assets install kokoro
simple-stt-ctl assets install tdt_ctc-110m-q8_0.gguf
simple-stt-ctl assets select tts --model kokoro --runtime sherpa-cpu
simple-stt-ctl assets remove kokoro
```

An asset cannot be removed while it or one of its dependencies is assigned.

Credentials are kept in the OS credential vault, not in JSON config or command
arguments. Set a credential from a pipe or a named environment variable; the CLI
never prints its value:

```text
printf '%s' "$SIMPLE_STT_AI_API_KEY" | simple-stt-ctl credentials set ai --stdin
simple-stt-ctl credentials set ocr --env OCR_API_KEY
simple-stt-ctl credentials set stt --env SIMPLE_STT_STT_API_KEY
simple-stt-ctl credentials list
simple-stt-ctl credentials remove ai
```

The `stt` credential is independent from the `ai` cleanup credential. The
legacy `simple-stt-ctl config-show` output includes `reader_read_along`, the
remote STT provider/fallback/timeout keys, and these numeric-second reader
budgets: `request_budgets.connect_timeout_secs=3`,
`request_budgets.queue_timeout_secs=30`,
`request_budgets.cold_model_load_timeout_secs=180`,
`request_budgets.warm_model_load_timeout_secs=30`,
`request_budgets.warm_inference_timeout_secs=60`,
`request_budgets.stream_inactivity_timeout_secs=60`, and
`request_budgets.total_timeout_secs=300`. The complete JSON values are
available through `config --scope reader show` and `config --scope reader
defaults`.

Settings also keeps STT, TTS, and OCR client connections independent. Each has
its own **Save connection** and **Clear key** controls. Saving one connection
preserves other unsaved settings; clearing its key leaves the URL and the other
features' keys intact. A blank key field retains an existing saved key until
Clear key is pressed.

Model and voice suggestions refresh after a 600 ms pause in URL/key edits.
Refresh icons beside their fields retry discovery. Discovery uses GET requests,
never inference; a missing model catalog does not prevent entering a model ID.
The usual base URL is `http://100.85.156.101:8765/v1`; a matching full endpoint
such as `/v1/audio/speech` is also accepted without duplicating its path. Custom
prefixes and query parameters are preserved. STT permits HTTP for private IP
networks, including Tailscale, and requires HTTPS for public hosts.
See [provider client settings research](research/provider-client-settings.md)
for the source-backed URL and discovery contract.

`simple-stt-ctl serve` starts the installed headless reader API controller in the
foreground. It does not require the capture service or a desktop session.

`simple-stt-ctl media transcribe` returns only the transcript (or the requested
JSON object) on stdout. It shares the configured capture/remote STT path and
does not type or deliver the result into the active application.
