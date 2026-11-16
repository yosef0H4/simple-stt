# Read-along target selection and advancement

Research and reproduction: 2026-11-15.

## Recorded evidence

Installed RPM 21 logged `selected window is no longer foreground; refusing global input` during page advancement, `OCR failed: no readable text found in the selected region`, and `selected region is outside its window`. The supplied 13.88-second recording shows OBS and its recursive preview throughout the sampled frames, so browser interaction is not visible in that clip.

A read-only live KWin probe reproduced selection of an overlapping video-player window while the browser was active. The production live Rust regression then failed because the selected UUID differed from the active window UUID. It passed after changing the queried list to actual visual stacking order.

## Primary-source comparison

- KWin [windowList implementation](https://github.com/KDE/kwin/blob/master/src/scripting/workspace_wrapper.cpp) returns the general windows list, distinct from its stacking-order accessor. The [KWin API](https://develop.kde.org/docs/plasma/kwin/api/) documents `workspace.stackingOrder` with later entries covering earlier ones. Reversing `windowList()` therefore cannot establish which overlapping window is visible. Use stackingOrder and exclude other desktops/activities and desktop/panel/popup windows.
- Old [TTS-anywhere](https://github.com/yosef0H4/TTS-anywhere) binds the foreground HWND and saves a window-relative rectangle (`src/electron/main.ts`, setup around 2907). Its Windows-only `sendHotkeyToWindow` uses `PostMessageW` (`services/nodehotkey/src/send.ts`, around 336). That permits background addressing, which cannot be carried over as global Wayland keyboard injection. Its advance loop waits, captures, normalizes OCR text and retries bounded unchanged/empty pages (main.ts around 3055–3151).
- The [Wayland keyboard protocol](https://wayland.freedesktop.org/docs/html/apa.html) routes keys to the focused surface. The [RemoteDesktop portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html) supplies user-authorized keyboard input and persistent restore tokens; it has no target-window parameter for keyboard notification. It recommends EIS, which the shared local input broker now uses for key events and Unicode text. Reusing one authorized session avoids process launch and repeated authorization for every page.
- [LunaTranslator OCR scheduling](https://github.com/HIllya51/LunaTranslator/blob/main/docs/en/ocrparam.md) documents image stability, input-triggered delays, optional foreground gating and text-similarity suppression. This supports bounded transition recaptures and explicit focus handling; its Windows capture mechanisms are not Linux background-input APIs.

## Agreed behavior

Finish current audio after focus loss; keep the selected window and wait for its return, then continue automatically. Do not steal focus. Preserve configured one-page prefetch, advance delay and bounded repeated Space behavior. Briefly retry empty OCR from the same window region without additional key presses, then show the actual cause. Distinguish waiting from failure in the HUD.

Keyboard page turns on KDE Wayland use a keyboard-only RemoteDesktop session, authorized before starting speech, retained for subsequent turns, with replacement restore tokens stored privately. Every turn rechecks the exact selected window; cancellation interrupts permission waits and focus waits. Region clicks now request a separate pointer-only RemoteDesktop session and use EIS absolute input regions. Unsafe or unavailable region mappings fail explicitly. Page-turn keys support named navigation keys, letters, digits, F1–F12 and modifier chords.

## Verification

The desktop harness supports `--overlap --focus-change --transient-empty --unchanged-once`: a newer background window must not receive input or be captured; focus loss must wait; the first Space intentionally has no effect; a later turn deliberately shows a blank frame; actual second-page OCR and speech must follow. A separate `--no-prefetch` run checks visibility timing. User settings/models stay unchanged in isolated test profiles.

Repeated native runs exposed a separate KWin query-script lifecycle failure. Queries now have unique plugin names, unload only their own plugin, and serialize load/run/unload across our processes with a private runtime lock, waiting for deletion. KWin currently derives IDs from its script-list size and schedules deletion asynchronously ([source](https://github.com/KDE/kwin/blob/master/src/scripting/scripting.cpp)); concurrent loading/deletion is therefore avoided. Valid returned state is retained if cleanup has already happened.
