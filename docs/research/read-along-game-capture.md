# Read-along capture for games on KDE Wayland

Research note for selecting a fixed dialogue-box region once and capturing each
new page. No commercial game was launched or tested for this note; Ace Attorney
behavior remains unverified.

## What the current capture paths do

### KDE Wayland

Simple STT selects a visible KWin window and stores the selected rectangle in
that window's logical frame coordinates (`reader/crates/readerd/src/read_along_capture.rs:151-176`).
The current KWin query uses `workspace.stackingOrder`, filters to the current
desktop/activity, and returns window IDs and frame geometry
(`reader/crates/readerd/src/read_along_capture/kde_backend.rs:184-207`).

For each capture, the backend calls KWin ScreenShot2 `CaptureWindow` with
`native-resolution=true`, `include-decoration=true`, and
`include-shadow=false`, then crops the selected region after scaling from KWin
logical frame dimensions to screenshot pixel dimensions
(`reader/crates/readerd/src/read_along_capture/kde_backend.rs:294-365`). KWin's
implementation maps those options to screenshot flags. Its window capture path
gets the compositor's primary render device, allocates a GL texture/framebuffer,
renders the requested window scene item, and reads the pixels back
([option mapping](https://github.com/KDE/kwin/blob/master/src/plugins/screenshot/screenshotdbusinterface2.cpp#L104-L128),
[window render path](https://github.com/KDE/kwin/blob/master/src/plugins/screenshot/screenshot.cpp#L192-L253)).
That is a compositor-rendered *window* capture, rather than a crop from the
visible desktop image. The KWin implementation derives image size from the
window's target scale; it does not special-case the `native-resolution` flag in
the window-specific render function.

This design is a good fit for a fullscreen game whose dialogue is part of a
KWin-managed window surface: it avoids unrelated windows covering the selected
dialogue region in the screenshot. It also avoids relying on a game's own
`WM_PRINT` handler. This is an inference from the render path, not an Ace
Attorney test result.

**Fullscreen/direct-scanout limit:** KWin's source proves that ScreenShot2
attempts to render the selected scene item into an offscreen GL target. It does
not establish that every Vulkan/OpenGL/Proton surface remains capturable during
every direct-scanout, Gamescope, protected-content, driver, or compositor state.
The game's presented surface must be represented by a renderable KWin window
item for this path to succeed. KWin logs and a real capture of the specific
game are needed to distinguish an unsupported surface from a bad window/region
selection. A Gamescope session is a different compositor and is outside this
KWin backend.

Keyboard advancement uses a keyboard-only XDG RemoteDesktop portal session;
the portal requires permission, and Simple STT checks the selected KWin window
is active before sending keys. When focus changes, the intended behavior is to
wait for that same window rather than steal focus (`portal_keyboard.rs:42-61,
95-130`; `kde_backend.rs:391-404,461-465`; `docs/architecture.md:229`). This
means a game must be focused to receive Space/Enter/Right. The portal is not a
background-window message API. Page-turn input can work with a fullscreen game
without clicking the dialogue box, provided the game retains keyboard focus
and its own controls advance dialogue with the configured key.

### Windows capture path

The current Windows helper is **not a BitBlt implementation**. It calls
`PrintWindow(hwnd, dc, 2)` (`reader/assets/windows-read-along.ps1:25,52-67`).
Microsoft documents `PrintWindow` as asking the window-owning application to
render itself into a supplied DC via `WM_PRINT` or `WM_PRINTCLIENT`
([PrintWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-printwindow)).
That contract depends on the target application handling that request; a
successful call is not a documented guarantee that a custom GPU swapchain has
been drawn into the DC. For a game, a blank or incomplete result is therefore
plausible, but not universal.

`BitBlt` is a separate GDI operation. Microsoft documents that `CAPTUREBLT`
includes layered windows above the source window, and that the source DC
determines copied color data
([BitBlt](https://learn.microsoft.com/en-us/windows/desktop/api/wingdi/nf-wingdi-bitblt)).
That is not an API guarantee for all full-screen GPU swapchains. Do not describe
the current helper as failing because it uses BitBlt; it does not.

Microsoft's Desktop Duplication API is more explicit about games: it does not
depend on the app's graphics API and can duplicate full-screen DirectX apps, but
protected video content is protected against access
([Desktop Duplication](https://learn.microsoft.com/en-us/windows-hardware/drivers/display/desktop-duplication-api)).
Windows.Graphics.Capture has a supported window/display capture model with a
system picker and active-capture border
([screen capture](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture)).
Those APIs are Windows alternatives, not evidence that GDI or KWin capture is
universally broken.

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
