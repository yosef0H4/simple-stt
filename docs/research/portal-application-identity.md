# XDG Desktop Portal application identity

Research checked against the upstream portal documentation on 2026-11-15.

## Findings

The XDG Desktop Portal host `Registry.Register` interface lets an unsandboxed
host application associate its D-Bus peer with an application ID. The ID must
match the basename of a desktop entry, and registration must happen before
that D-Bus peer makes any portal method call. The registry allows registration
once per peer. The upstream documentation recommends re-registering if the
portal service restarts. [Registry interface](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.host.portal.Registry.html)

The portal API reference says host registration replaces automatic cgroup
application detection and can improve attribution when applications do not
launch through the usual desktop path. It also says host registration may be
deprecated in the future, so clients must tolerate the interface being absent.
The same portal APIs are used by different desktop backends, making this the
standard cross-desktop route rather than a KDE notification customization.
[Portal API reference](https://flatpak.github.io/xdg-desktop-portal/docs/api-reference.html)

KDE documents that portal permission decisions are associated with an app ID
obtained from the Registry or a desktop-environment-managed systemd unit. Its
host-app guidance recommends a real app ID rather than an empty one, which can
match too broadly. This identity improves the attribution shown for a request;
it does not grant or pre-approve desktop control. [KDE portal permission guidance](https://develop.kde.org/docs/administration/portal-permissions/)

## Implementation

`readerd` uses the same `io.github.yosef0H4.simple_stt` app ID as the main
Simple STT shell and ensures a matching desktop entry exists before registering
that ID through ashpd. Registration happens before it starts portal
integrations. The installed entry uses the Simple STT name and icon.
Development installs create a user desktop entry with the same identity when
needed. Registration
errors are logged and the service continues; on older or unavailable portal
registries, the desktop may continue to identify the application through its
normal mechanisms or display it as unidentified. No portal permission is
silently accepted, and KDE-specific permission tables or warning suppression
are not used.

The registry documentation recommends re-registration after the portal
service restarts. The current client registers at `readerd` desktop startup;
portal restart recovery remains dependent on the desktop's normal app
attribution mechanisms.
