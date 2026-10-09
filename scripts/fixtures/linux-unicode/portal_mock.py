#!/usr/bin/env python3
"""Private xdg-desktop-portal RemoteDesktop mock for isolated KWin E2E."""
import asyncio
import os
from pathlib import Path
import subprocess
import sys
import threading

from dbus_next import BusType, Message, MessageType, Variant
from dbus_next.aio import MessageBus
from dbus_next.service import ServiceInterface

SERVICE = "org.freedesktop.portal.Desktop"
PATH = "/org/freedesktop/portal/desktop"
IFACE = "org.freedesktop.portal.RemoteDesktop"
REQUEST_IFACE = "org.freedesktop.portal.Request"
SESSION = "/org/freedesktop/portal/desktop/session/simple_stt/e2e"

class PrivatePortal:
    def __init__(self, bus, injector):
        self.bus = bus
        self.injector = injector
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

    async def finish_request(self, message, token, results):
        path = self.request_path(message.sender, token)
        print("REPLY", message.sender, path, token, flush=True)
        await self.bus.send(Message.new_method_return(message, "o", [path]))
        await asyncio.sleep(0.025)
        await self.bus.send(Message(destination=message.sender, path=path,
            interface=REQUEST_IFACE, member="Response", message_type=MessageType.SIGNAL,
            signature="ua{sv}", body=[0, results]))
        print("SENT", path, flush=True)

    def handle(self, message):
        if message.message_type != MessageType.METHOD_CALL or message.path != PATH:
            return None
        if message.interface == IFACE:
            print("CALL", message.sender, message.member, message.signature, flush=True)
        if message.interface == IFACE:
            member = message.member
            if member == "CreateSession":
                asyncio.create_task(self.finish_request(message, "createsession", {"session_handle": Variant("s", SESSION)}))
                return True
            if member == "SelectDevices":
                asyncio.create_task(self.finish_request(message, "selectdevices", {}))
                return True
            if member == "Start":
                asyncio.create_task(self.finish_request(message, "start", {"restore_token": Variant("s", "private-e2e-token")}))
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

async def main():
    injector = str(Path(sys.argv[1]).resolve())
    bus = await MessageBus(bus_type=BusType.SESSION).connect()
    await bus.request_name(SERVICE)
    portal = PrivatePortal(bus, injector)
    # Register the path/interface so D-Bus property and introspection requests
    # from the real helper receive normal replies; the user handler below owns
    # the portal methods so it can retain each caller's unique bus name.
    bus.export(PATH, ServiceInterface(IFACE))
    bus.add_message_handler(portal.handle)
    print("READY", flush=True)
    await asyncio.get_running_loop().create_future()

if __name__ == "__main__":
    asyncio.run(main())
