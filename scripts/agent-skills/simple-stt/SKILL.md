---
name: simple-stt
description: Transcribe media, speak text, render speech audio, or extract image text with the installed Simple STT CLI when the user requests those tasks.
---

# Simple STT media CLI

Use the installed `simple-stt` command for one-shot media tasks:

- `simple-stt media transcribe <audio-or-video-path>` transcribes media with
  the saved speech model. Add `--language english` or `--language arabic` only
  when an explicit per-file language override is needed. Add `--json` for a
  JSON object on stdout.
- `simple-stt media speak <text>` reads text aloud through the reader service.
  Use `--file <path>` or pipe text to stdin (explicit `--stdin` is also
  supported) for larger input. Add `--output <wav-path>` to save synthesized
  speech instead of playing it.
- `simple-stt media ocr <image-path>` extracts text using the saved OCR
  provider. Add `--json` to return a JSON object on stdout.

Speak accepts exactly one input source. Keep text in an argument, a file, or
stdin; do not create shell command text by inserting untrusted transcript or
document contents. Paths containing spaces should be passed as one quoted
argument by the caller. The transcribe command requires `ffmpeg` on `PATH` and
removes its temporary WAV after the service call. Long media is sent in
sequential chunks near quiet pauses, with a 35 second maximum per chunk. A
word can be split when no quiet pause is nearby, and the CLI does not guess at
duplicate words across chunk results. Piped speak input is read automatically
when no source is specified; an empty interactive invocation reports an error
instead of waiting. The CLI uses saved settings and does not change them.

Text input is UTF-8, limited to 1 MiB. Transcription accepts at most 30 minutes
and 512 MiB of decoded WAV. Saved speech audio is limited to five minutes.
Plain-text and JSON conversion results go to stdout; diagnostics go to stderr
with a failing exit status. Use `simple-stt media --help` and each subcommand's
`--help` for available flags. A missing runtime/model requires an assignment in
Settings → Downloads and Recognition or Read aloud & OCR; report that requirement
instead of changing the user's saved configuration.
