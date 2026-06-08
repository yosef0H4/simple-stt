# Recording overlay and notices

The persistent Rust capture service owns the fast Win32 overlay. AutoHotkey owns tray notices and dialogs; it does not repaint the high-frequency recording visualizer.

## Primary states

```text
Hidden
Recording       rec ....||||||....
Transcribing   Transcribing...
```

A transient second line may be layered over the primary state:

```text
rec ....||||||....
Loading speech model…
```

## Notices

```text
Loading speech model…
No speech detected
Recording too short
Speech engine failed — see log
Audio service error — see log
```

Routine worker idle shutdown is log-only unless diagnostic-overlay behavior is enabled. Final typing notices are emitted by the AHK shell because foreground checks and `SendText()` live there.

Implementation:

```text
src/capture/overlay.rs
src/capture/overlay_windows.rs
src/bin/uvox_capture.rs
ahk/uvox.ahk
```
