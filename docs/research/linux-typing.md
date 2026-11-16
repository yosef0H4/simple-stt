# Linux typing delivery

The Linux typing path uses a 450 WPM default. The speed slider places 450 WPM at its midpoint and reserves its rightmost position for Instant. Instant disables pacing through the existing `paced_typing_enabled` setting; it does not select a paste delivery mode. Saved finite speeds remain available when pacing is re-enabled.

The implementation converts WPM to a target delay using five characters per word. At 450 WPM this is an integer delay of 26 ms per character; Instant uses zero configured delay. Actual throughput depends on the target application, accessibility calls, backend overhead, scheduling, and character composition. A configured speed is not a guarantee of observed throughput.

## Clipboard-free Unicode typing

For the Auto and Native backends, the native Linux helper first attempts accessible text insertion with `--type-text --typing-delay-ms`. It inserts text through AT-SPI in paced units without using the clipboard. This requires an accessible, editable focused target; it is not a general guarantee for every Wayland application or terminal. The helper and its caller monitor cancellation, and a failed or interrupted attempt that may have inserted a prefix must not be replayed through a different delivery method.

If accessible insertion is unavailable before any text has been sent, the configured typing backend may be attempted. If no safe backend is available, the delivery path reports that typing is unavailable. Clipboard behavior in that case follows the user's existing clipboard-preservation setting; it is not an automatic paste of the transcript.

## ydotool timing and text limits

The examined Fedora installation provides ydotool 1.0.4. Its pinned upstream [type implementation](https://raw.githubusercontent.com/ReimuNotMoe/ydotool/v1.0.4/Client/tool_type.c) and [client dispatch](https://raw.githubusercontent.com/ReimuNotMoe/ydotool/v1.0.4/Client/ydotool.c) show that `--key-delay` is a delay after a key down/up pair. A separate `--key-hold` delay occurs between down and up and defaults to 20 ms. With the default hold left in place, a nominal 26 ms delay would yield at least about 46 ms per ordinary character, before processing overhead. The fallback explicitly sets `--key-hold 0` to avoid adding that hidden hold to the selected delay.

The same upstream implementation treats command-line arguments as escape sequences by default. The fallback passes `--escape 0` to preserve literal backslashes. Its text mapping is a fixed ASCII-to-physical-key table based on a US layout; it is not a Unicode text input interface and must not be used for Arabic UTF-8. A successful process exit alone does not establish that the intended characters reached the application.

The fallback rejects non-ASCII or unsupported control characters and requires a configured basic US layout before it sends keys. This configuration check is a **reject-only heuristic**, not proof of the effective keymap or modifier state: custom keymaps, XKB options, environment or compositor defaults, layout changes, and held modifiers can change the result. It does not make ydotool suitable for Unicode. Aborting an active child terminates it; a partial attempt is not retried automatically.

## wtype and KDE layout information

[wtype's pinned 0.4 source](https://raw.githubusercontent.com/atx/wtype/v0.4/main.c) maps Unicode characters to XKB keysyms and requires the `zwp_virtual_keyboard_manager_v1` Wayland protocol. Its presence on disk does not establish that the running compositor offers the protocol or that a particular target handles the generated keymap correctly. The upstream [compatibility issue](https://github.com/atx/wtype/issues/45) includes historical reports of failures on Plasma/KWin; these reports are not a universal claim about current KDE versions.

KWin's [layout D-Bus implementation](https://raw.githubusercontent.com/KDE/kwin/master/src/keyboard_layout.cpp) serializes `getLayoutsList()` entries as `(shortName, displayName, longName)`. The second field is a user-facing display name, not an XKB variant. KWin's [XKB implementation](https://raw.githubusercontent.com/KDE/kwin/master/src/xkb.cpp) can load configured layout and variant lists, locale settings, environment defaults, or a fallback keymap. Therefore an index matched to `kxkbrc` layout and variant entries can reject known unsafe configurations but cannot by itself prove the effective physical mapping.

## Validation status

Private GTK/X11 checks passed for paced Unicode insertion at 450 and 850 WPM, Instant, exact English/Arabic/emoji content, caret progression and unchanged binary clipboard data. Controlled-child Rust tests cover Abort killing and reaping the typing process, and prevent fallback after a partial attempt. Private GTK checks also passed for stopping during focus loss and process abort: no redirected text, continuing insertion, or clipboard change was observed. Native focus transitions on real Wayland apps remain unverified. The private compositor test harness is separate from the user's desktop; no host typing test is implied by these source findings. In particular, no claim is made that all Wayland focus transitions, terminals, or editors support the accessibility path.


## Measured timing — 2026-11-04

Three runs of the actual installed ydotool client sent 25 public ASCII characters
to a private discard-only Unix datagram socket. No ydotool daemon, uinput device,
compositor or host desktop received input. These timings isolate client pacing;
they do not include a real editor's processing time.

| Setting | Old median dispatch | Fixed median dispatch | Fixed median gap |
| --- | ---: | ---: | ---: |
| 450 WPM | 1137.5 ms | 631.9 ms | 26.16 ms |
| 850 WPM | 848.7 ms | 345.9 ms | 14.24 ms |
| Instant | 510.0 ms | 3.5 ms | 0.03 ms |

The same private-socket check verified that `--escape 0` preserves a literal
backslash followed by n instead of emitting Enter. Raw events and timings are
retained in ignored `artifacts/combined-app/typing/ydotool-timing.json`.

On a separate private X server and accessibility bus, an actual GTK entry
received `Hello مرحبا 🙂 ` exactly. The native helper completed its 14-codepoint
payload in 354.6 ms at the 26 ms delay, 200.2 ms at 14 ms, and 15.4 ms with zero
delay. The custom non-text clipboard object remained unchanged in every case.
These are individual functional checks, not a broad editor benchmark.
