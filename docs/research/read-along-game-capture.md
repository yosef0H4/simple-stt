# Game screen capture on Windows and Linux

Scope covers Read Screen, OCR Copy, and selecting a dialogue box once for
Read-along or Repeat. The Windows findings are from source review and API
documentation. See the recorded KDE fixture result below. Ace Attorney and
native Windows exclusive fullscreen remain unverified.

## What the current capture paths do

### KDE Wayland

Simple STT selects a rectangle, finds the visible KWin window at its center,
then stores the rectangle relative to that window's logical frame. The KWin
query uses `workspace.stackingOrder` on the current desktop and activity.
[Window query](../../reader/crates/readerd/src/read_along_capture/kde_backend.rs#L225-L247),
[selection](../../reader/crates/readerd/src/read_along_capture/kde_backend.rs#L478-L512),
[saved target](../../reader/crates/readerd/src/read_along_capture.rs#L196-L211).

For each capture, the backend calls KWin ScreenShot2 `CaptureWindow` with
`native-resolution=true`, `include-decoration=true`, and
`include-shadow=false`, then crops the selected region after scaling from KWin
logical frame dimensions to screenshot pixel dimensions.
The compositor uses the window's target scale and renders its scene item into
an offscreen buffer. This is a window capture and can exclude overlapping
windows. [Simple STT capture](../../reader/crates/readerd/src/read_along_capture/kde_backend.rs#L335-L419),
[KWin window rendering](https://github.com/KDE/kwin/blob/master/src/plugins/screenshot/screenshot.cpp#L192-L281).

Wayland compositors can scan out a fullscreen client's buffer directly. KWin
still needs a renderable window item for its screenshot path, and that path
honors windows excluded from capture. The practical inference is that KDE has
a suitable API for fullscreen games it manages. Vulkan, Proton, direct-scanout
states, and separate Gamescope sessions need their own checks.
[Wayland compositor model](https://wayland.freedesktop.org/docs/book/Compositors.html),
[KWin window rendering](https://github.com/KDE/kwin/blob/master/src/plugins/screenshot/screenshot.cpp#L192-L281).

Keyboard advancement uses a keyboard-only XDG RemoteDesktop portal session;
the portal requires permission, and Simple STT checks the selected KWin window
is active before sending keys. When focus changes, the intended behavior is to
wait for that same window rather than steal focus (`portal_keyboard.rs:42-61,
95-130`; `kde_backend.rs:391-404,461-465`; `docs/architecture.md:229`). This
means a game must be focused to receive Space/Enter/Right. The portal is not a
background-window message API. Page-turn input can work with a fullscreen game
without clicking the dialogue box, provided the game retains keyboard focus
and its own controls advance dialogue with the configured key.

### Other Linux desktops

On X11, Simple STT binds a window but reads the selected pixels from the root
drawable through `GetImage`. Overlap can therefore change which pixels appear
in the region. Outside KDE on Wayland, one-off Read Screen and OCR Copy use
Lumine when installed and fall back to the interactive Screenshot portal.
Saving a target for Repeat and Read-along needs a window-bound backend. A
future portable Wayland approach could request a ScreenCast window stream
where the portal backend offers one, then crop the authorized PipeWire stream.
[X11 capture](../../reader/crates/readerd/src/read_along_capture/linux_backend.rs#L135-L165),
[desktop support](../../reader/crates/readerd/src/read_along_capture.rs#L174-L194),
[one-off capture](../../reader/crates/readerd/src/region_capture.rs#L38-L69),
[XDG Screenshot](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Screenshot.html),
[XDG ScreenCast](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html).

### Windows

The picker freezes a frame before showing its window. It captures a fullscreen
foreground window by HWND and remembers its identity and bounds before the game
can lose focus. The picker draws from that captured frame and translates the
selection back to the original pixels if the display mode changes.
[Windows picker](../../reader/crates/readerd/src/read_along_capture/windows_picker.rs)

Windows Graphics Capture supplies the same frame pipeline for a single-monitor
preview and saved-window capture. Short-lived helper processes keep a capture
failure from stopping the reader. Windows 10 version 1903 added the Win32 window
and monitor targeting APIs. Capturing by window keeps a covering app out of
Repeat and Read Along. GDI covers a virtual desktop spanning several monitors
and serves as a compatibility fallback. A monitor-based fullscreen recapture
requires the original game to be foreground and its selected region uncovered.
[Frame pipeline](../../reader/crates/readerd/src/read_along_capture/windows_graphics.rs),
[Windows Graphics Capture](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture),
[CreateForWindow](https://learn.microsoft.com/en-us/windows/win32/api/windows.graphics.capture.interop/nf-windows-graphics-capture-interop-igraphicscaptureiteminterop-createforwindow),
[CreateForMonitor](https://learn.microsoft.com/en-us/windows/win32/api/windows.graphics.capture.interop/nf-windows-graphics-capture-interop-igraphicscaptureiteminterop-createformonitor)

A game in true exclusive fullscreen can lose focus or minimize when a desktop
picker appears. Borderless fullscreen allows the compositor to draw overlays
and remains the workaround for a game that prevents capture or restores
exclusive presentation while the picker is open. Windows Fullscreen
Optimizations can present a game that requests exclusive mode through an
optimized borderless path.
[Microsoft Fullscreen Optimizations](https://devblogs.microsoft.com/directx/demystifying-full-screen-optimizations/)

The test fixture exercises region capture before OCR, then verifies a saved
region under a covering window. Run it with mock OCR and speech using
`python scripts/test-read-along-windows.py --fullscreen --feedback read-region --require-fullscreen-graphics`.
A Direct3D test window can also request exclusive mode. Test actual games
separately because Direct3D's reported swap chain state does not rule out
Fullscreen Optimizations.

## Game-reading application references

- [LunaTranslator OCR settings](https://github.com/HIllya51/LunaTranslator/blob/main/docs/en/ocrparam.md)
  documents periodic OCR, waiting for image stability, and a mouse/keyboard
  trigger followed by a configurable delay/stability wait. It suppresses
  repeated OCR output with a text-similarity threshold. For a bound game window,
  trigger-driven execution is documented to run only while that game is
  foreground.
- [LunaTranslator window screenshot help](https://github.com/HIllya51/LunaTranslator/blob/main/docs/en/fastkeys.md)
  says a bound-window screenshot tries GDI first and then Windows.Capture.
  This documents a Windows capture strategy, not a native KDE Wayland backend.
- [Game2Text](https://github.com/mathewthe2/Game2Text) documents selecting a
  game/application window, dragging a text region, and OCR extraction. Its
  README lists Windows 10 and macOS as supported desktop platforms, and also
  describes text hooking for visual novels. Its docs do not establish a Linux
  Wayland or arbitrary fullscreen-game capture path.

These tools support the product pattern—bind a game, save a dialogue ROI, scan
after input/render delay, and avoid speaking duplicate OCR—but their documented
Windows capture methods do not prove GPU capture works for every game.

## Practical next validation

On Windows, `python scripts/test-read-along-windows.py --read-screen` and its
`--fullscreen` variant exercise the picker on a private Tk window, including
startup time, cancellation, and whether the OCR crop comes from the frozen
preview. Add a GPU-presenting fixture with separate borderless and true
exclusive modes for the capture change. Start from the game hotkey, inspect
the preview and OCR a known line, then refocus the same game and verify that
Repeat reads the changed line. Test a second monitor and a focus change during
selection or before advancing. Require the selected game crop or a clear
failure, with no OCR or key sent to the competing window. Compare hotkey-to-
preview latency against the current ordinary-window baseline.
[Windows test contract](../testing.md#recorded-actions),
[Windows harness](../../scripts/test-read-along-windows.py#L78-L86).

The user-facing workflow can remain: focus the game, select the dialogue box
once, then read one page and advance with the configured key. For a bounded
noncommercial fixture, a GTK3 `Gtk.GLArea` window can draw high-contrast text in
a lower-screen dialogue panel and change the text on Space. The checkout host
has GTK3, Cairo, and epoxy development packages available, but no SDL2
development package or PyOpenGL/pygame module. Under KDE Wayland, use a managed
window first, then fullscreen, and repeat with an overlapping window. This can
validate KWin window discovery, offscreen `CaptureWindow`, ROI scaling, OCR,
and portal key delivery without claiming Ace Attorney compatibility. A
Vulkan-presenting fixture would be a separate stretch test because a GL
fixture does not establish Proton/Vulkan behavior. This note did not install
dependencies or run a fixture.

## Executed GPU dialogue test

On this Fedora KDE host, `/tmp/simple-stt-openai-e2e-venv/bin/python scripts/test-read-along-desktop.py --game-dialogue --fullscreen` passed actual region selection, KWin window capture, Paddle OCR, Kokoro speech, a RemoteDesktop Space press, and second-page speech. The fixture rendered its dialogue texture through OpenGL on AMD Radeon Graphics (radeonsi/renoir), rather than a GTK text label. It does not establish Ace Attorney, Proton/Vulkan, Gamescope or native Windows compatibility.
