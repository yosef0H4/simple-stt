# Tooltip activity and notification overlay

Uvox uses the existing Win32 tracking tooltip as a lightweight activity surface. It is deliberately not a replacement for the log file.

## Display rules

- The first line shows the active user-facing operation: the recording visualizer, `Transcribing...`, or `Typing...`.
- A second line is added only when a short notice is useful during an active operation.
- When Uvox is otherwise idle, a notice may appear by itself and expire automatically.
- Only one notice is displayed at a time. Errors replace warnings and informational notices; warnings replace informational notices. Repeated identical notices do not extend their lifetime indefinitely.
- Starting a new recording clears stale notices from the previous session.
- Automatic idle model unload remains log-only to avoid unnecessary interruptions.

## User-facing notices

| Situation | Tooltip text |
| --- | --- |
| Model context is loading | `Loading speech model...` |
| Model context is ready | `Speech model ready` |
| Recording is too short | `Recording too short` |
| Transcription returned no text | `No speech detected` |
| Focus changed before insertion | `Text not inserted: active window changed` |
| Focus changed during insertion | `Typing stopped: active window changed` |
| Transcription failed | `Transcription failed - see log` |
| Model load failed | `Speech model failed - see log` |
| Microphone stream failed | `Microphone error - see log` |
| Text injection failed | `Typing failed - see log` |
| Hotkey toggled | `Hotkey enabled` or `Hotkey disabled` |
| Settings reloaded | `Settings applied` |
| Tray model test started | `Testing speech model...` |
| Tray model test passed | `Speech model test passed` |
| Tray model test failed | `Speech model test failed - see log` |

## Implementation map

- `src/overlay.rs`: overlay primary states, notice priority, expiration, deduplication, and multiline rendering.
- `src/main.rs`: activity transitions and user-facing notices.
- `src/transcript.rs`: typing completion and rejection events so the overlay hides only after insertion completes.
- `src/audio.rs`: microphone stream error events for a visible failure notice.

## Manual Windows checklist

1. Start Uvox with a cold model. Hold the record hotkey and confirm that the visualizer remains on line one while `Loading speech model...` appears on line two.
2. Release after speaking. Confirm the overlay changes to `Transcribing...`, then `Typing...`, then disappears after text insertion.
3. Tap the hotkey too briefly and confirm `Recording too short` appears briefly.
4. Record silence and confirm `No speech detected` appears briefly.
5. Change focus before transcription finishes and confirm text is not inserted and the focus-change notice appears.
6. Configure a short idle timeout. Confirm the model unload appears in logs only. Record again and confirm `Loading speech model...` appears while the context reloads.
7. Disable and re-enable the hotkey from the tray and confirm the brief notices.
8. Use the tray model test and confirm its start, success, and failure notices.

## Relevant Win32 guidance

- Multiline tooltips: https://learn.microsoft.com/en-us/windows/win32/controls/implement-multiline-tooltips
- Updating tooltip text: https://learn.microsoft.com/en-us/windows/win32/controls/ttm-updatetiptext
- Notification UX guidance: https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/app-notifications-ux-guidance
- Warning-message guidance: https://learn.microsoft.com/en-us/windows/win32/uxguide/mess-warn
