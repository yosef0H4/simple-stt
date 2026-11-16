# XDG RemoteDesktop input default

Research: 2026-11-15. Reviewed the supplied `ChatGPT-Linux_Speech_To_Text_Apps.md`.

The report recommends complete-text clipboard delivery plus a compositor-authorized paste shortcut. This avoids relying on per-character physical key mappings for multilingual dictation. Its specific claims about future Plasma/Fedora versions are not required by this implementation and are not treated as verified compatibility guarantees.

The [official RemoteDesktop specification](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html) defines CreateSession → SelectDevices → Start. Request keyboard access only; Start can show a permission dialog. The same specification documents persistent permission mode 2 and single-use restore tokens: save the replacement token returned by Start. Revoked or unavailable grants can prompt again.

The specification supports D-Bus Notify methods and recommends EIS. Current
Simple STT uses ConnectToEIS in the local reader controller and never mixes
Notify methods into that session. The old C helper still permits portal-only
shortcuts for isolated regression tools, and provides read-only app/selection
observation; its XTest/uinput/accessibility-write paths were removed.

## Application behavior

The UI selects delivery actions, with legacy JSON backend values normalized to
auto. Keyboard sessions are shared across dictation typing, paste/copy chords
and read-along page turns. Click actions request pointer permission separately.
Single-use restore tokens are atomically replaced in private files. Denied
requests never choose a different injector. The broker is local IPC only;
headless HTTP inference does not initialize desktop input.

EIS `ei_text.utf8` avoids reverse-mapping transcript Unicode through physical
keycodes. It requires compositor text capability (libei protocol 1.6); older
desktops report typing unavailable and still allow portal paste actions. KWin
routes committed text to focused text-input surfaces, with a keysym fallback
when no text-input client is active. This is foreground input. Kitty enables
Wayland IME by default and forwards commits to its child; game controls use
keyboard key events because gameplay need not activate text entry.

Sources: [libei sender API](https://libinput.pages.freedesktop.org/libei/api/group__libei-sender.html),
[KWin EIS](https://github.com/KDE/kwin/blob/master/src/plugins/eis/eiscontext.cpp),
[KWin input methods](https://github.com/KDE/kwin/blob/master/src/inputmethod.cpp),
[Kitty configuration](https://sw.kovidgoyal.net/kitty/conf.html).

## Validation

`scripts/test-portal-input-e2e.py` exercises the actual broker against an isolated
KWin compositor through a test portal FD relay. The test controls only its own
private D-Bus session, runtime directory and synthetic target windows. It never
auto-approves the user's real desktop portal. The read-along desktop test also
covers capture, actual OCR/TTS, focus waiting and page advancement with a
separately authorized test permission token. See the test logs for the verified
application and compositor versions; older historical AT-SPI typing checks do
not validate the new EIS path.
