# AutoHotkey v2 research for the Simple STT shell

Research date: 2026-06-07. This document is documentation-first. All API claims below were checked against the official AutoHotkey v2 documentation. Community snippets were not used as implementation authorities.

## Entry-point policy

Every executable `.ahk` script begins with:

```ahk
#Requires AutoHotkey v2.0
#SingleInstance Force
```

`#Requires` lets the launcher reject the wrong interpreter version. `#SingleInstance Force` replaces an existing shell instance rather than opening a duplicate shell.

Official sources:

- `#Requires`: <https://www.autohotkey.com/docs/v2/lib/_Requires.htm>
- `#SingleInstance`: <https://www.autohotkey.com/docs/v2/lib/_SingleInstance.htm>
- v1.1 to v2 changes: <https://www.autohotkey.com/docs/v2/v2-changes.htm>

## APIs used by the shell

| API | Relevant v2 behavior | Simple STT approach |
| --- | --- | --- |
| `Hotkey()` | Creates, updates, enables, or disables runtime hotkeys. The v2 function form is used instead of v1 command syntax. | `HotkeyManager.Configure()` disables old bindings and installs new key-down and key-up bindings at runtime. |
| `HotIf()` and `#HotIf` | Context-sensitive variants are available. Function-created hotkeys inherit the current HotIf criterion. | Researched but not needed for the first shell: Simple STT performs explicit state checks in short callbacks. |
| Custom combinations | `a & b` syntax creates a custom combination where the prefix key has special handling. Custom combinations act as wildcard matches already, so do not prepend `*` to the combination. Prefix keys need deliberate tap-preservation behavior. | `CapsLock+S` becomes `CapsLock & s` and `CapsLock & s up`; separate wildcard CapsLock down/up callbacks preserve an ordinary tap. |
| Key-down and key-up hotkeys | The `up` suffix defines release behavior. Wildcards and the hook prefix affect matching and recursion. | Hold-to-record installs a down callback and an `up` callback; non-CapsLock hotkeys use `$*` to accept extra modifiers and avoid retriggering from generated input. |
| `KeyWait()` | Waits for key state transitions; in v2 it returns false on timeout and true otherwise. | Researched for manual tests; not used inside shell hotkey callbacks because callbacks remain non-blocking. |
| `Send()` | Sends keys using the current send mode and interprets braces/modifier syntax. | Not used for transcript content. |
| `SendText()` | Sends literal text rather than treating characters as key syntax. | Chosen for Unicode transcript chunks. |
| `SendInput()` | Fast input method with limitations when blocked by another process or integrity boundary. | Researched but not selected for literal transcript content; `SendText()` is clearer for Unicode text. |
| `SendMode()` | Changes the default method used by `Send`. | Not needed for `SendText()`-based transcript typing. |
| `SetKeyDelay()` | Applies to SendEvent/ControlSend, not SendInput. | Not used. Simple STT controls pacing with a one-shot `SetTimer()` between `SendText()` chunks. |
| `A_TrayMenu` | Built-in tray menu object. | The shell deletes default entries and creates the complete Simple STT tray menu. |
| `Menu` objects | v2 object API for menu manipulation. | Used through `A_TrayMenu.Add()`, `.Delete()`, and `.Default`. |
| `TraySetIcon()` | Sets the tray icon. | Shell owns the icon. A stock shell icon is used until a product icon is packaged. |
| `TrayTip()` | Displays tray notification balloons/toasts. v2 parameter order is text, title, options. | Researched but no longer used; shell notices stay in logs or the Rust overlay instead of Windows notification toasts. |
| `Gui()` | Constructs a GUI object. Controls and callbacks use object APIs in v2. | Settings are entirely implemented in `ahk/lib/SettingsGui.ahk`. |
| GUI controls and events | Controls expose properties such as `.Value`/`.Text`; callbacks are registered with `OnEvent`. | GUI buttons dispatch only quick local work or asynchronous helper commands. |
| `SetTimer()` | Repeated or negative one-shot timers schedule callbacks. | Drives helper completion polling, service event polling, service supervision, and paced transcript chunks. |
| `Run()` | Can return a created process PID through an output variable. | The shell records the exact capture PID and each one-shot helper PID. |
| `ProcessExist()` | Checks whether a process or PID exists. | PID-based monitoring only; no name-wide termination. |
| `ProcessWait()` | Waits for a process to exist. | Researched; readiness is instead checked asynchronously by state-file publication plus `PING`. |
| `ProcessWaitClose()` | Waits for a process to close and accepts a timeout. | Graceful capture shutdown waits before fallback termination. |
| `ProcessClose()` | Terminates a process. | Used only as the final exact-PID fallback after graceful shutdown timeout. |
| `OnExit()` | Registers exit callbacks. | Shell stops helper timers and closes the capture service on shell exit. |
| `OnError()` | Registers unhandled-error callbacks. | Shell logs uncaught callback errors. |
| `OnMessage()` | Registers Windows-message callbacks. | Researched for `WM_COPYDATA`; not selected for production IPC. |
| `FileOpen()` and file objects | v2 uses file objects for reads, writes, and flushes. Pipe names can be opened as paths, but a direct pipe client would still add Win32 edge cases on the UI side. | Shell uses small UTF-8 response files written by disposable helpers. Rust owns socket and stream handling. |
| `DllCall()` | Calls native functions with explicit types. The DLL/function separator is a single backslash. | Used only for `SystemFunction036` random token generation in the shell. Avoided for direct IPC. |
| `Persistent()` | Keeps the script alive after its startup thread completes; v2 also automatically persists scripts with hotkeys, timers, or a GUI. | The shell calls `Persistent` explicitly after initialization for clarity. |
| `FileCreateShortcut()` and `A_Startup` | A shortcut in the Startup folder is the documented simple login-start option. | `ApplyStartupRegistration()` creates or deletes `A_Startup\Simple STT.lnk`, targeting the script in development and the bundled AutoHotkey runtime plus script in distribution. |
| String escaping | AutoHotkey uses the backtick as its escape character; a backslash is an ordinary literal character. | Path strings use one backslash. The helper tab codec explicitly maps one backslash to two for transport. |
| `InputHook()` | v2 replacement for older input command patterns. | Hotkey recorder captures a final non-modifier key while sampling physical modifiers. |

Official API pages:

- <https://www.autohotkey.com/docs/v2/lib/Hotkey.htm>
- <https://www.autohotkey.com/docs/v2/lib/HotIf.htm>
- <https://www.autohotkey.com/docs/v2/Hotkeys.htm>
- <https://www.autohotkey.com/docs/v2/lib/KeyWait.htm>
- <https://www.autohotkey.com/docs/v2/lib/Send.htm>
- <https://www.autohotkey.com/docs/v2/lib/SetKeyDelay.htm>
- <https://www.autohotkey.com/docs/v2/lib/Menu.htm>
- <https://www.autohotkey.com/docs/v2/lib/TraySetIcon.htm>
- <https://www.autohotkey.com/docs/v2/lib/TrayTip.htm>
- <https://www.autohotkey.com/docs/v2/lib/Gui.htm>
- <https://www.autohotkey.com/docs/v2/lib/SetTimer.htm>
- <https://www.autohotkey.com/docs/v2/lib/Run.htm>
- <https://www.autohotkey.com/docs/v2/lib/ProcessExist.htm>
- <https://www.autohotkey.com/docs/v2/lib/ProcessWait.htm>
- <https://www.autohotkey.com/docs/v2/lib/ProcessWaitClose.htm>
- <https://www.autohotkey.com/docs/v2/lib/ProcessClose.htm>
- <https://www.autohotkey.com/docs/v2/lib/OnExit.htm>
- <https://www.autohotkey.com/docs/v2/lib/OnError.htm>
- <https://www.autohotkey.com/docs/v2/lib/OnMessage.htm>
- <https://www.autohotkey.com/docs/v2/lib/FileOpen.htm>
- <https://www.autohotkey.com/docs/v2/lib/DllCall.htm>
- <https://www.autohotkey.com/docs/v2/lib/Persistent.htm>
- <https://www.autohotkey.com/docs/v2/lib/FileCreateShortcut.htm>
- <https://www.autohotkey.com/docs/v2/Program.htm>
- <https://www.autohotkey.com/docs/v2/lib/InputHook.htm>

## AltGr and modifier handling

The official hotkey documentation describes AltGr as generally equivalent to left Ctrl plus right Alt, represented by `<^>!`. The shell accepts an explicit `AltGr` token and maps it to that symbol pair. Physical validation treats AltGr as right Alt held physically. Generic and left/right-specific Ctrl, Alt, Shift, and Win tokens are retained separately. This needs manual testing on an actual AltGr keyboard layout because synthesized layout behavior cannot be validated in this Linux editing environment.

Source: <https://www.autohotkey.com/docs/v2/Hotkeys.htm#AltGr>

## Typing choice and integrity boundaries

Simple STT sends literal transcript chunks with `SendText()` and checks the active foreground HWND before every chunk. It never defaults to clipboard injection. AHK-generated text is protected from dictation retriggering by hook-prefixed hotkeys for ordinary modifier chords and by separating the CapsLock custom-combination path.

AutoHotkey documents that sending may be ineffective when a target process runs at a higher integrity level. Simple STT does not try to bypass UAC or protected application boundaries. Distribution may optionally document UI Access as an advanced packaging decision, but it is not silently enabled.

Source: <https://www.autohotkey.com/docs/v2/lib/Send.htm>

## Pipes, file objects, and UI responsiveness

A direct AHK named-pipe implementation was prototyped on paper but rejected: opening and reading a pipe from the shell would either block a callback or require more native API plumbing. AHK callbacks stay short. The chosen shell IPC launches `simple-stt-ctl.exe` with `Run()`, records its PID, and polls process completion with `SetTimer()`. The helper handles JSON, the loopback socket, and UTF-8 response-file writes.

## Distribution runtime

Development can run `ahk/simple-stt.ahk` with AutoHotkey v2 installed. Distribution packages the readable AHK scripts together with `AutoHotkey64.exe` and launches the script explicitly. End users do not need a separate AHK installation, and the release avoids an opaque compiled AHK blob.

Official sources:

- <https://www.autohotkey.com/docs/v2/Program.htm>

## Startup registration decision

Two common per-user Windows options were considered:

1. HKCU `Software\\Microsoft\\Windows\\CurrentVersion\\Run`
2. a shortcut in `A_Startup`

The shell chooses a per-user `A_Startup\\Simple STT.lnk` shortcut through `FileCreateShortcut()`. This visibly targets the AHK shell in development or the bundled `AutoHotkey64.exe` plus script path in distribution and avoids carrying forward the old monolith registry registration code. Packaging must launch the AHK shell, not a Rust executable.
