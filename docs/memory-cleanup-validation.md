# Memory-cleanup validation

## Why the architecture changed

The retired monolith called a native unload path inside the resident Rust process. That may release model state and VRAM but cannot guarantee that every allocator page is returned to Windows. The new cleanup guarantee is worker process exit.

Only `simple-stt-infer.exe` imports `libloading` and loads `parakeet.dll`. `simple-stt-capture.exe` remains resident but does not load the model DLL. After the idle timeout or manual unload, capture asks the worker to shut down, waits for its PID to exit, and force-terminates only that child PID if it hangs past `worker_shutdown_grace_ms`. An independent atomic PID tracker keeps that fallback available even when a transcript read is blocked behind the supervisor mutex.

## Repeatable Windows diagnostic

Run after building release binaries and making the runtime/model available:

```powershell
.\scripts\memory-cleanup-validation.ps1 -IdleSeconds 5
```

Optional parameters:

```powershell
.\scripts\memory-cleanup-validation.ps1 -IdleSeconds 5 -ShellPid <pid> -KeepProcesses
```

The script:

1. chooses an isolated temporary schema-v2 config;
2. records optional shell RAM;
3. launches `simple-stt-capture.exe` with an exact PID and random token;
4. records baseline capture RAM;
5. requests a model smoke test through `simple-stt-ctl.exe`;
6. polls service state until the infer-worker PID is visible;
7. records infer RAM;
8. invokes `nvidia-smi` when available;
9. records post-transcription process data;
10. requests `unload-model`;
11. confirms the infer-worker PID exits;
12. verifies the worker RAM record disappears with process exit;
13. records capture RAM after cleanup;
14. records VRAM snapshot after exit;
15. saves JSON evidence under `artifacts\memory-cleanup-validation-<timestamp>.json`.

The smoke test uses `fixtures\parakeet-smoke.wav`; it exercises the same disposable worker model-load boundary without requiring microphone interaction. A manual dictation pass should be run as an additional product check.

## Acceptance criteria

```text
infer worker launches lazily
infer worker has measurable RAM while model is loaded
infer worker PID exits after unload or idle period
Get-Process no longer returns worker RAM after exit
capture service remains alive
capture service WorkingSet after cleanup remains near its lightweight baseline
nvidia-smi shows model VRAM released after worker exit, when available
```

“Near baseline” is intentionally observational rather than a hardcoded byte limit because CPAL, Windows audio buffers, logging, and allocator behavior vary across systems. Any large sustained increase in capture RAM after several cycles is a bug to investigate.

## Observed results in the editing environment

Not measured. The source-only Linux editing environment does not have Windows processes, Cargo-built executables, CUDA, a Parakeet runtime, or `nvidia-smi`. It cannot truthfully produce RAM or VRAM numbers.

What was verified statically:

```text
active capture modules do not import ParakeetNative or libloading
active crate imports libloading only in src/infer/parakeet_native.rs
capture supervisor sends graceful Shutdown before exact-child-PID fallback
mock-worker integration source exercises the blocked-read force path
worker logs that process exit is the memory cleanup guarantee
```

The Windows diagnostic script is included so observed machine-specific figures can be captured rather than invented.

After ordinary completion, STT, voice, and OCR workers remain reusable until their respective configured idle timeouts. Explicit unload, cancellation, zero timeout, and operating-system memory-pressure handling can release workers earlier. Timers and worker identities prevent stale callbacks from unloading a newer use or process.

## Residency regression scope

Validate with controllable worker children that STT, voice, API voice, and OCR reuse their workers before the configured timeout and exit after it, including a worker whose memory usage exceeds prior fixed thresholds. Check model-switch cache bounds, warm leases, zero timeout, explicit unload, and stale timer callbacks against the exact child PID. Exercise screen selection with a delayed and canceled picker, including cancellation while OCR prewarm holds its gate. Include an end-to-end memory pass on a supported desktop; native Windows behavior and its process-memory measurement are not validated by Linux tests.

## Windows validation checkpoint — 2026-11-16

- Master debd49c patched, Windows installer rebuilt and installed. Client and background reader server run from the installed directory; authenticated API at port 8765. Settings/model backups preserved. Schema 8 migrated to 9; all legacy shared settings unchanged except intentional CUDA selection, with English/Arabic model choices preserved.
- Built Parakeet v0.5.0 ABI 6 from source with CUDA 12.8 for RTX 5080 (sm_120a). English/Arabic passed through worker and installed HTTP STT endpoint. Old DLLs backed up under ignored artifacts/windows-validation.
- Fixed Windows shell startup warning, recorded-action cancellation modal, bundled/per-language runtime routing, single-model auto-assignment, TTS canonical paths and ONNX DLL selection, Settings Reload draft race/navigation scroll, and Windows capture authentication discovery through profile/PID-scoped Credential Manager entries.
- Full scripts/test-full.cmd passed, including actual recording prewarm/unload, controlled English/Arabic keyboard selection, hello-world typing and full-format clipboard restoration. After the final capture-authentication hotfix, credential scoping and live English/Arabic server STT passed; the entire suite was not rerun after that hotfix.
- Installed server Kokoro/Supertonic WAV synthesis and OCR passed. Installed client automatic English/Arabic voice routes rendered WAVs successfully. All 37 Windows TTS/OCR asset entries downloaded and verified. All 34 OCR models loaded and inferred; 32 passed the numeric probe (normalizing Unicode digits). Older Cyrillic v3 and Tamil v3 misread the probe; language-specific quality remains unvalidated.
- Recorded-action native timing/modifier/cancel/focus/release checks passed. Controller read-along backpressure/recovery tests and screenshot settling after macro completion passed; full Windows desktop read-along/Resume/Next, physical spoken dictation/hold chords, startup/reload desktop pass, and TTS/OCR CUDA remain unvalidated.
- Evidence and reproducible probes: artifacts/windows-validation. Installer: artifacts/dist/simple-stt-setup.exe. Source changes remain uncommitted; pre-existing DESIGN/architecture changes were preserved.

Background-server follow-up: the shared desktop daemon had stopped, leaving no listener. Restarted installed readerd in standalone `server` mode with its own sibling data directory (`background-server-reader`), the existing reader config, and SIMPLE_STT_EMBEDDED_READER=1 so installed assets and STT bridge remain available. This avoids desktop-client shutdown owning the API process. Authenticated model discovery, Kokoro, Supertonic, and actual OCR passed; localhost and Tailscale-address probes return expected unauthenticated HTTP 401. API key unchanged. No reboot auto-start was configured.

## Windows tooltip and hotkey checkpoint — 2026-11-17

- Updated master to f8d8024; preserved prior Windows fixes and pre-existing design/architecture edits. The pre-update stash and artifacts/windows-validation/pre-tooltip-update.patch remain as backups.
- Shared Rust overlay model owns phase mapping, localized content and native glyph fallback. Shortcut selection/formatting uses one shared implementation; platform adapters retain OS drawing/window work. Windows no longer reactivates/resizes unchanged tooltips every 2 ms; uses ~30 Hz recording updates and disables tooltip fade/animation. English/Arabic screenshots inspected; both live probes passed with no hidden recording samples.
- Measured a 890 ms status-helper launch on a 969 ms paste. Added a persistent authenticated Rust status relay, ordered delivery feedback, parent-process lifetime handling, queue cleanup, and in-memory timing aggregation. Real-service paste regression passed at 62–109 ms; blocked Edit probing decreased from 1000 ms to about 100 ms. Delayed consumption, newer clipboard copies, non-text clipboard objects, modifiers, cancellation, focus safety and held-key release passed.
- scripts/test-full.cmd and cargo test --workspace --all-features passed after shared tooltip/relay changes; formatting and strict Clippy passed. Later default-only change passed 150 root library tests, Settings selection API, real Rust/browser contracts, strict Clippy and static checks. Fresh profiles now assign only Record and Cancel; all reading and secondary shortcuts default to None. Saved bindings are preserved.
- Rebuilt installer and portable package after the default change; package now includes action-replay.ahk. No installation or publishing occurred. Running source executables were renamed to *.before-hotkey-defaults while rebuilding so the user client/server could continue running.
- CUDA English/Arabic passed; CPU/Vulkan English/Arabic passed with matching transcripts. Independent updated readerd server PID 29436 remains running on 0.0.0.0:8765, with sibling background-server-reader data and existing reader config/assets/key. Verified authenticated localhost and Tailscale models, Kokoro/Supertonic WAVs, OCR, and English/Arabic STT. Removed only an obsolete installed capture-state file whose PID did not exist, retaining a dated backup; this repaired post-reboot API STT fallback.
- User reports development desktop behavior works correctly. Remaining: native Linux visual/runtime pass, full physical microphone quality/hardware-chord checklist, and full desktop read-along Resume/Next recovery. A separate local CLI WAV-render probe timed out; background HTTP TTS/OCR/STT passed. Its test parent was stopped; no successful local CLI render is claimed. Server is not configured to auto-start after reboot.
- Next action if local CLI rendering is revisited: isolate Windows named-pipe/ensure-daemon startup and inherited-handle behavior against the real controller, using a private profile and bounded process-tree cleanup. Do not replace the working background server or user settings while diagnosing it.

## Windows integration checkpoint

- Integrated GitHub master at 6bcd4ed. Windows and Linux share reader routing, language selection, fallback, status text and read-along recovery. The Windows adapter uses profile-specific pipes, physical screen coordinates, and an independently managed background API.
- Native Windows tests verified clipboard delivery at 78â€“109 ms, recorded-action timing and focus safeguards, picker cancellation, one-page lookahead, and Resume/Next recovery after the failure notice dismisses. Private HTTP tests verified server authentication, client/shutdown isolation and separate TTS and OCR keys with real local fallback after HTTP 503.
- English and Arabic dictation matched across CPU, Vulkan, and CUDA runs using the downloaded runtimes. Both TTS engines and PaddleOCR passed on CPU and CUDA. All 34 OCR recognizers ran on CUDA. Two older recognizers misread the numeric sample. Tamil v3 passed a Tamil sample and Cyrillic v3 dropped the same character on CPU and CUDA. Cyrillic v5 passed the Cyrillic sample.
- Published the source-built Windows Blackwell CUDA runtime with SHA-256 and upstream notices in the Simple STT runtimes repository. Tested the actual public download with NVIDIA dependencies in an empty Windows profile. Setup and the portable ZIP leave inference runtimes and model installation to Settings. The user's installed profile is separate from the test profiles.
- Final Simple STT 0.3.1 installer passed the wizard and isolated install checks. The complete Windows and workspace suites passed. The installed package repeated English and Arabic CUDA inference and real TTS/OCR fallback with separate credentials. Test servers and inference workers exited.
- Windows native region picker verified in the installed package. It opened in 84 ms in the borderless fixture, compared with 1,034 ms for the previous picker. The tooltip stayed hidden and the preview stayed stable across sampled drag frames. Two local screen reads reused the OCR and TTS workers that loaded while the picker was open. The full Windows suite passed, including the unified General shortcuts.
- Read Along now warms both workers during selection. On one dense Windows CPU fixture, time to first audio fell from 7.37 to 4.75 seconds. The recorded-action fixture reached next-page OCR 560 ms after modifier release, down from 969 ms with the same 500 ms settle delay. CPU and CUDA OCR runs played the next page from the one-page lookahead. Matched Linux latency and click-to-advance in a real target remain unverified.
- Native English and Arabic tooltip captures show localized Read Along shortcuts, readable passages, and next-page status. The Windows tray smoke test verifies the branded tray icon and native color bitmaps for each menu item. Settings, Rust, and AutoHotkey share the desktop label catalog.
- The shared Settings save bar stays centered over the content at 360 px in Arabic. The browser suite exercised both languages, both themes, locale save and reload, and the six current Settings pages.
- Models now shows Mode first, followed by language route cards and nested local fallbacks. Browser checks verified separate speech credentials and saved language, model, and voice choices after reopening. OCR uses one remote provider with per-language local models.
- Windows desktop fixtures showed OCR progress within 222 ms of selection release while OCR was still pending. Copy Screen displayed Text copied after updating the clipboard. Read Screen and Read Selection showed speech progress before audio arrived. The full Windows suite and Linux static checks passed. Native Linux tooltip timing still needs a desktop check.
- The packaged Windows Settings test verified a single PID for Tauri and the Settings API, window restore on relaunch, Save after renderer suspension and capture restart, and both close actions. Closing the window exited the Settings and WebView2 processes while capture stayed running. Explicit browser and remote administration checks passed without model inference.
- The shared emoji shortcut grid now renders in native Windows tooltips. English and Arabic captures showed the configured icons in order, and the rebuilt package showed the compact footer through Show Shortcuts. The recording tooltip stayed visible across 107 samples. Linux static checks passed. No models were loaded.
- Packaged native Settings shows save feedback at the bottom left and closes through the title bar. The browser retains connection status and Close. A mock transcription failure confirmed an old secondary endpoint received no request and the enabled local fallback ran. Browser route checks and model-free speech fallback tests passed.
- Packaged Windows tests verified Caps Lock tap and hold, an alternate hold chord, preservation of both Caps Lock states during shortcuts, and release without replacing an active notice. The shortcut recorder accepts a bare Caps Lock only for Show Shortcuts. Browser tests exercised the grouped shortcut fields. Read Selection, Read Screen, Read Along and clipboard-source fixtures sent cleaned text to mock speech; no models loaded.
- Native Linux desktop, real sign-in startup, physical spoken microphone quality, and rotating the live server credential remain outside the verified results.
