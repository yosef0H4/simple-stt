#!/usr/bin/env python3
"""Private xdg-desktop-portal RemoteDesktop mock for isolated KWin E2E."""
import asyncio
import os
from pathlib import Path
import subprocess
import sys
import threading

from dbus_next import BusType, Message, MessageType, Variant
from dbus_next.constants import PropertyAccess
from dbus_next.aio import MessageBus
from dbus_next.service import ServiceInterface, dbus_property, method

SERVICE = "org.freedesktop.portal.Desktop"
PATH = "/org/freedesktop/portal/desktop"
IFACE = "org.freedesktop.portal.RemoteDesktop"
REQUEST_IFACE = "org.freedesktop.portal.Request"
SESSION_IFACE = "org.freedesktop.portal.Session"
KW_SOURCE = "org.kde.KWin"
KW_PATH = "/org/kde/KWin/EIS/RemoteDesktop"
KW_IFACE = "org.kde.KWin.EIS.RemoteDesktop"


def option(options, name, default=None):
    value = options.get(name, default)
    return value.value if isinstance(value, Variant) else value

class PrivatePortal:
    def __init__(self, bus, injector):
        self.bus = bus
        self.injector = injector
        self.sessions = {}
        self.injection_lock = threading.Lock()
        self.injection_client = subprocess.Popen(
            [injector, "--stream"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, env=os.environ.copy(), text=True, bufsize=1)
        self.keycode_client = subprocess.Popen(
            [injector, "--stream-keycode"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, env=os.environ.copy(), text=True, bufsize=1)

    def request_path(self, sender, token):
        name = (sender or "").lstrip(":").replace(".", "_")
        return f"/org/freedesktop/portal/desktop/request/{name}/{token}"

    def session_path(self, sender, token):
        name = (sender or "").lstrip(":").replace(".", "_")
        return f"/org/freedesktop/portal/desktop/session/{name}/{token}"

    def request_options(self, message):
        index = {"CreateSession": 0, "SelectDevices": 1, "Start": 2}.get(message.member)
        return message.body[index] if index is not None else {}

    async def finish_request(self, message, results):
        options = self.request_options(message)
        token = option(options, "handle_token")
        if not token:
            raise RuntimeError(f"{message.member} omitted its handle_token")
        path = self.request_path(message.sender, token)
        print("REPLY", message.sender, path, token, flush=True)
        await self.bus.send(Message.new_method_return(message, "o", [path]))
        await asyncio.sleep(0.025)
        await self.bus.send(Message(destination=message.sender, path=path,
            interface=REQUEST_IFACE, member="Response", message_type=MessageType.SIGNAL,
            signature="ua{sv}", body=[0, results]))
        print("SENT", path, flush=True)

    def handle(self, message):
        if message.message_type != MessageType.METHOD_CALL:
            return None
        if message.interface == SESSION_IFACE and message.member == "Close":
            self.close_session(message.path)
            return Message.new_method_return(message)
        if message.path != PATH:
            return None
        if message.interface == IFACE:
            print("CALL", message.sender, message.member, message.signature, flush=True)
            member = message.member
            if member == "CreateSession":
                options = message.body[0]
                token = option(options, "session_handle_token")
                if not token:
                    return Message.new_error(message, "org.freedesktop.DBus.Error.Failed", "missing session_handle_token")
                session = self.session_path(message.sender, token)
                if session in self.sessions:
                    return Message.new_error(message, "org.freedesktop.DBus.Error.Failed", "duplicate session handle")
                self.sessions[session] = {"sender": message.sender, "requested": 0, "granted": 0, "started": False}
                self.bus.export(session, PortalSession(self, session))
                asyncio.create_task(self.finish_request(message, {"session_handle": Variant("o", session)}))
                return True
            if member == "SelectDevices":
                session_path = message.body[0]
                options = message.body[1]
                session = self.sessions.get(session_path)
                if session is None:
                    return Message.new_error(message, "org.freedesktop.DBus.Error.Failed", "unknown session")
                session["requested"] = int(option(options, "types", 3)) & 7
                print("SELECT", session_path, "requested", session["requested"],
                      "restore", bool(option(options, "restore_token")), flush=True)
                asyncio.create_task(self.finish_request(message, {}))
                return True
            if member == "Start":
                session_path = message.body[0]
                session = self.sessions.get(session_path)
                if session is None:
                    return Message.new_error(message, "org.freedesktop.DBus.Error.Failed", "unknown session")
                session["granted"] = session["requested"]
                session["started"] = True
                print("START", session_path, "granted", session["granted"], flush=True)
                asyncio.create_task(self.finish_request(message, {
                    "devices": Variant("u", session["granted"]),
                    "restore_token": Variant("s", "private-e2e-token"),
                }))
                return True
            if member == "ConnectToEIS":
                asyncio.create_task(self.connect_to_eis(message))
                return True
            if member == "NotifyKeyboardKeysym":
                try:
                    keysym = int(message.body[2])
                    state = int(message.body[3])
                    print("KEY", f"U+{keysym - 0x01000000:04X}" if keysym > 0x01000000 else f"0x{keysym:X}", state, flush=True)
                    with self.injection_lock:
                        self.injection_client.stdin.write(f"{keysym} {state}\n")
                        self.injection_client.stdin.flush()
                        ready, _, _ = __import__("select").select([self.injection_client.stdout], [], [], 3)
                        if not ready or self.injection_client.stdout.readline().strip() != "OK":
                            detail = self.injection_client.stderr.readline().strip()
                            raise RuntimeError(detail or "persistent fake-input client did not acknowledge event")
                    return Message.new_method_return(message)
                except Exception as exc:
                    return Message.new_error(message, "org.freedesktop.DBus.Error.Failed", str(exc))
            if member == "NotifyKeyboardKeycode":
                try:
                    keycode = int(message.body[2])
                    state = int(message.body[3])
                    print("KEYCODE", keycode, state, flush=True)
                    with self.injection_lock:
                        self.keycode_client.stdin.write(f"{keycode} {state}\n")
                        self.keycode_client.stdin.flush()
                        ready, _, _ = __import__("select").select([self.keycode_client.stdout], [], [], 3)
                        if not ready or self.keycode_client.stdout.readline().strip() != "OK":
                            detail = self.keycode_client.stderr.readline().strip()
                            raise RuntimeError(detail or "persistent keycode client did not acknowledge event")
                    return Message.new_method_return(message)
                except Exception as exc:
                    return Message.new_error(message, "org.freedesktop.DBus.Error.Failed", str(exc))
        return None

    async def connect_to_eis(self, message):
        session_path = message.body[0]
        session = self.sessions.get(session_path)
        if not session or not session["started"] or not session["granted"]:
            await self.bus.send(Message.new_error(
                message, "org.freedesktop.DBus.Error.Failed", "session not started or no devices granted"))
            return
        capabilities = session["granted"]
        print("EIS", session_path, "capabilities", capabilities, flush=True)
        reply = await self.bus.call(Message(
            destination=KW_SOURCE,
            path=KW_PATH,
            interface=KW_IFACE,
            member="connectToEIS",
            signature="i",
            body=[capabilities],
        ))
        if reply.message_type == MessageType.ERROR:
            await self.bus.send(Message.new_error(
                message, "org.freedesktop.DBus.Error.Failed", str(reply.body)))
            return
        if len(reply.body) < 2 or not reply.unix_fds:
            await self.bus.send(Message.new_error(
                message, "org.freedesktop.DBus.Error.Failed", f"KWin returned no EIS fd: {reply.signature}"))
            return
        fd_index, cookie = reply.body[0], reply.body[1]
        if not isinstance(fd_index, int) or fd_index < 0 or fd_index >= len(reply.unix_fds):
            await self.bus.send(Message.new_error(
                message, "org.freedesktop.DBus.Error.Failed", "KWin returned an invalid EIS fd handle"))
            return
        original = reply.unix_fds[fd_index]
        forwarded = os.dup(original)
        try:
            await self.bus.send(Message.new_method_return(
                message, "h", [0], unix_fds=[forwarded]))
        finally:
            os.close(forwarded)
            os.close(original)
        print("EIS_CONNECTED", session_path, "capabilities", capabilities, "cookie", cookie, flush=True)

    def close_session(self, path):
        self.sessions.pop(path, None)
        try:
            self.bus.unexport(path, SESSION_IFACE)
        except Exception:
            pass
        print("SESSION_CLOSE", path, flush=True)


class PortalInterface(ServiceInterface):
    def __init__(self):
        super().__init__(IFACE)

    @dbus_property(access=PropertyAccess.READ)
    def version(self) -> "u":
        return 2

    @dbus_property(access=PropertyAccess.READ)
    def AvailableDeviceTypes(self) -> "u":
        return 3


class PortalSession(ServiceInterface):
    def __init__(self, portal, path):
        super().__init__(SESSION_IFACE)
        self.portal = portal
        self.path = path

    @method()
    def Close(self):
        self.portal.close_session(self.path)

async def main():
    injector = str(Path(sys.argv[1]).resolve())
    bus = await MessageBus(bus_type=BusType.SESSION, negotiate_unix_fd=True).connect()
    await bus.request_name(SERVICE)
    portal = PrivatePortal(bus, injector)
    # Register the path/interface so D-Bus property and introspection requests
    # from the real helper receive normal replies; the user handler below owns
    # the portal methods so it can retain each caller's unique bus name.
    bus.export(PATH, PortalInterface())
    bus.add_message_handler(portal.handle)
    print("READY", flush=True)
    await asyncio.get_running_loop().create_future()

if __name__ == "__main__":
    asyncio.run(main())
