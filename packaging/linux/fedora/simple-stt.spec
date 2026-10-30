Name:           simple-stt
Version:        0.3.1
Release:        4%{?dist}
%global __provides_exclude_from ^/usr/lib/simple-stt/.*\.so.*$
Summary:        Offline speech-to-text dictation for Linux desktops
License:        GPL-2.0-only AND MIT AND OFL-1.1
URL:            https://github.com/yosef0H4/simple-stt
BuildArch:      x86_64
Requires:       alsa-lib
Requires:       libX11
Requires:       libXtst
Requires:       pango
Requires:       cairo
Requires:       vulkan-loader
Requires:       libgomp
Requires:       glib2
Requires:       at-spi2-core
Requires:       systemd
Requires:       xdg-utils
Requires:       libsecret
Requires:       xclip
Requires:       xdotool
Requires:       wl-clipboard
Requires:       wtype

%description
Simple STT provides local microphone dictation with an isolated inference
worker and an optional Vulkan-accelerated Parakeet speech runtime.

%prep

%build

%install
mkdir -p %{buildroot}/usr
cp -a %{_sourcedir}/stage/. %{buildroot}/

%files
%defattr(-,root,root,-)
%attr(0755,root,root) /usr/bin/simple-stt
%dir /usr/lib/simple-stt
%attr(0755,root,root) /usr/lib/simple-stt/simple-stt-linux
%attr(0755,root,root) /usr/lib/simple-stt/simple-stt-capture
%attr(0755,root,root) /usr/lib/simple-stt/simple-stt-infer
%attr(0755,root,root) /usr/lib/simple-stt/simple-stt-ctl
%attr(0755,root,root) /usr/lib/simple-stt/simple-stt-settings
%attr(0755,root,root) /usr/lib/simple-stt/linux-fast-paste
%dir /usr/lib/simple-stt/runtime
%dir /usr/lib/simple-stt/runtime/bin
%attr(0755,root,root) /usr/lib/simple-stt/runtime/bin/libparakeet.so
%attr(0644,root,root) /usr/lib/simple-stt/linux-installation
%dir /usr/share/applications
/usr/share/applications/io.github.yosef0H4.simple_stt.desktop
/usr/share/applications/simple-stt-settings.desktop
%dir /usr/share/icons/hicolor/scalable/apps
/usr/share/icons/hicolor/scalable/apps/simple-stt.svg
%dir /usr/lib/systemd/user
/usr/lib/systemd/user/simple-stt-linux.service
%license /usr/share/licenses/simple-stt/LICENSE
%license /usr/share/licenses/simple-stt/parakeet-MIT
%license /usr/share/licenses/simple-stt/THIRD_PARTY_NOTICES.md
%license /usr/share/licenses/simple-stt/OFL-JetBrainsMono.txt
%license /usr/share/licenses/simple-stt/OFL-NotoSansArabic.txt
%license /usr/share/licenses/simple-stt/parakeet-runtime.txt

%changelog
* Fri Oct 30 2026 Simple STT contributors - 0.3.1-4
- Avoid PRIMARY transfers in Smart Paste, report the correct delivery mode, and preserve final ASR tokens.

* Sat Oct 10 2026 Simple STT contributors - 0.3.1-1
- Prefetch and preserve the current clipboard, cancel obsolete transfers, add buffer notices, and wait for recording shortcut release before delivery.

* Fri Oct 09 2026 Simple STT contributors - 0.3.0-1
- Initial Fedora package
