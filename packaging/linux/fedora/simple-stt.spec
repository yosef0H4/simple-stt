Name:           simple-stt
Version:        0.3.1
Release:        29%{?dist}
%global __provides_exclude_from ^/usr/lib/simple-stt/.*\.so.*$
Summary:        Local dictation, read aloud, and screen text for Linux desktops
License:        GPL-2.0-only AND MIT AND OFL-1.1 AND CC-BY-SA-3.0
URL:            https://github.com/yosef0H4/simple-stt
BuildArch:      x86_64
Requires:       alsa-lib
Requires:       libX11
Requires:       pango
Requires:       cairo
Requires:       pipewire-libs
Requires:       glib2
Requires:       at-spi2-core
Requires:       google-noto-color-emoji-fonts
Requires:       systemd
Requires:       xdg-utils
Requires:       libsecret
Requires:       xclip
Requires:       wl-clipboard
Requires:       slurp

%description
Simple STT provides local microphone dictation, reading aloud, and screen text
recognition. Speech and OCR models and runtimes are optional downloads, with
isolated workers that unload independently after inactivity.

%prep

%build

%install
mkdir -p %{buildroot}/usr
cp -a %{_sourcedir}/stage/. %{buildroot}/

%files
/usr/lib/simple-stt/fixtures
%defattr(-,root,root,-)
%attr(0755,root,root) /usr/bin/simple-stt
/usr/bin/simple-stt-ctl
%dir /usr/lib/simple-stt
%attr(0755,root,root) /usr/lib/simple-stt/simple-stt-linux
%attr(0755,root,root) /usr/lib/simple-stt/simple-stt-capture
%attr(0755,root,root) /usr/lib/simple-stt/simple-stt-infer
%attr(0755,root,root) /usr/lib/simple-stt/simple-stt-ctl
%attr(0755,root,root) /usr/lib/simple-stt/simple-stt-settings
%attr(0755,root,root) /usr/lib/simple-stt/linux-fast-paste
%attr(0755,root,root) /usr/lib/simple-stt/readerd
%attr(0755,root,root) /usr/lib/simple-stt/readerctl
%attr(0755,root,root) /usr/lib/simple-stt/reader-tts
%attr(0755,root,root) /usr/lib/simple-stt/reader-ocr
%attr(0755,root,root) /usr/lib/simple-stt/reader-hud
%attr(0644,root,root) /usr/lib/simple-stt/downloads.json
%attr(0644,root,root) /usr/lib/simple-stt/lid.176.ftz
%attr(0644,root,root) /usr/lib/simple-stt/linux-installation
%dir /usr/share/applications
/usr/share/applications/io.github.yosef0H4.simple_stt.desktop
/usr/share/applications/simple-stt-settings.desktop
/usr/share/applications/simple-stt-readerd.desktop
%dir /usr/share/icons/hicolor/scalable/apps
/usr/share/icons/hicolor/scalable/apps/simple-stt.svg
%dir /usr/lib/systemd/user
/usr/lib/systemd/user/simple-stt-linux.service
/usr/lib/systemd/user/simple-stt-server.service
%license /usr/share/licenses/simple-stt/LICENSE
%license /usr/share/licenses/simple-stt/THIRD_PARTY_NOTICES.md
%license /usr/share/licenses/simple-stt/OFL-JetBrainsMono.txt
%license /usr/share/licenses/simple-stt/OFL-NotoSansArabic.txt
%license /usr/share/licenses/simple-stt/LICENSE-CC-BY-SA-3.0.txt
%license /usr/share/licenses/simple-stt/fastText-ATTRIBUTION.md

%changelog
* Thu Nov 05 2026 Simple STT contributors - 0.3.1-13
- Use the same shared runtime names for read-aloud and image text
- Add all supported Paddle text recognizers with verified per-model bundles

* Thu Nov 05 2026 Simple STT contributors - 0.3.1-12
- Fix pinned NVIDIA dependency extraction and native library discovery
- Show selections above separate runtime and model install sections
- Select compatible empty choices after verified downloads

* Wed Nov 04 2026 Simple STT contributors - 0.3.1-10
- Add optional verified runtime downloads and integrated read-aloud/OCR workers
- Keep the base package free of inference runtimes and model weights

* Sun Nov 01 2026 Simple STT contributors - 0.3.1-9
- Add readable shortcut keycaps, standard emoji and on-demand shortcut tooltips
- Keep Retry release hints visible and track typing/pasting progress

* Fri Oct 30 2026 Simple STT contributors - 0.3.1-4
- Avoid PRIMARY transfers in Smart Paste, report the correct delivery mode, and preserve final ASR tokens.

* Sat Oct 10 2026 Simple STT contributors - 0.3.1-1
- Prefetch and preserve the current clipboard, cancel obsolete transfers, add buffer notices, and wait for recording shortcut release before delivery.

* Fri Oct 09 2026 Simple STT contributors - 0.3.0-1
- Initial Fedora package
