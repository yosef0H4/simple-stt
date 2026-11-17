#!/usr/bin/env python3
"""Check real portal session cleanup on a private D-Bus bus, without desktop input."""
import argparse
import asyncio
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


async def mock(state):
    fixture = load('portal_fixture', ROOT / 'scripts/fixtures/linux-unicode/portal_mock.py')
    class RejectingPortal(fixture.PrivatePortal):
        def __init__(self, bus):
            self.bus = bus
            self.sessions = {}
            self.created = 0
            self.closed = 0
            self.started = 0
            self.holding = False
        def mode(self):
            path = state.with_suffix('.mode')
            return path.read_text() if path.exists() else 'reject_start'
        async def finish_request(self, message, results):
            if message.member == 'CreateSession' and self.mode() == 'delay_create':
                await asyncio.sleep(0.3)
            await super().finish_request(message, results)
        def save(self):
            temporary = state.with_suffix('.tmp')
            temporary.write_text(json.dumps({'created': self.created, 'closed': self.closed, 'live': len(self.sessions), 'started': self.started, 'holding': self.holding}))
            temporary.replace(state)
        def handle(self, message):
            if message.interface == fixture.IFACE and message.member == 'Start':
                if self.mode() == 'hold_start':
                    self.holding = True
                    self.save()
                    return True
                if self.mode() != 'eis_failure':
                    return fixture.Message.new_error(message, 'org.freedesktop.DBus.Error.Failed', 'controlled startup failure')
                self.started += 1
            if message.interface == fixture.IFACE and message.member == 'ConnectToEIS':
                return fixture.Message.new_error(message, 'org.freedesktop.DBus.Error.Failed', 'controlled EIS connection failure')
            before = len(self.sessions)
            result = super().handle(message)
            if len(self.sessions) > before:
                self.created += 1
            if message.interface == fixture.IFACE:
                self.save()
            return result
        def close_session(self, path):
            self.closed += 1
            self.holding = False
            super().close_session(path)
            self.save()
    bus = await fixture.MessageBus(bus_type=fixture.BusType.SESSION).connect()
    portal = RejectingPortal(bus)
    bus.export(fixture.PATH, fixture.PortalInterface())
    bus.add_message_handler(portal.handle)
    await bus.request_name(fixture.SERVICE)
    portal.save()
    await asyncio.get_running_loop().create_future()


def inside(root):
    helpers = load('portal_helpers', ROOT / 'scripts/test-portal-input-e2e.py')
    state = root / 'sessions.json'
    data = root / 'data'
    data.mkdir()
    env = {**os.environ, 'HOME': str(root), 'XDG_CONFIG_HOME': str(root / 'config'),
           'XDG_DATA_HOME': str(root / 'share'), 'XDG_CACHE_HOME': str(root / 'cache'),
           'SIMPLE_STT_READER_DATA': str(data), 'SIMPLE_TTS_CONFIG': str(root / 'config.json')}
    for key in ['REIS_DEBUG', 'DISPLAY', 'WAYLAND_DISPLAY', 'SIMPLE_STT_TEST_PORTAL_EIS_SOCKET']:
        env.pop(key, None)
    daemon = None
    daemon_log = None
    with (root / 'portal.log').open('w') as log:
        portal = subprocess.Popen([sys.executable, __file__, '--mock', str(state)], env=env, stdout=log, stderr=subprocess.STDOUT)
        try:
            helpers.wait_for(state.exists, 'private portal ready')
            daemon, daemon_log = helpers.start_daemon(env, root / 'config.json', data, root / 'daemon.log')
            for kind in ['prepare', 'prepare_pointer'] * 3:
                reply, _ = helpers.send_input(data, {'kind': kind})
                assert not reply.get('ok'), 'controlled rejection unexpectedly succeeded'
            mode = state.with_suffix('.mode')
            mode.write_text('eis_failure')
            for kind in ['prepare', 'prepare_pointer']:
                reply, _ = helpers.send_input(data, {'kind': kind})
                assert not reply.get('ok'), 'controlled EIS failure unexpectedly succeeded'
            assert json.loads(state.read_text())['started'] == 2
            for policy, expected in [('hold_start', 9), ('delay_create', 10)]:
                mode.write_text(policy)
                request_id = policy
                with ThreadPoolExecutor(max_workers=1) as pool:
                    result = pool.submit(helpers.send_input, data, {'kind': 'prepare'}, request_id)
                    helpers.wait_for(lambda: json.loads(state.read_text())['created'] == expected, 'pending session')
                    if policy == 'hold_start':
                        helpers.wait_for(lambda: json.loads(state.read_text())['holding'], 'pending consent')
                    assert helpers.ipc(data, {'op': 'cancel_input', 'id': request_id}).get('ok')
                    reply, _ = result.result(timeout=5)
                    assert not reply.get('ok'), 'canceled setup unexpectedly succeeded'
            try:
                helpers.wait_for(lambda: json.loads(state.read_text())['closed'] == 10, 'all sessions explicitly closed', timeout=3)
            except RuntimeError:
                raise AssertionError(f'portal sessions leaked: {json.loads(state.read_text())}') from None
            counts = json.loads(state.read_text())
            assert counts['created'] == counts['closed'] == 10 and counts['live'] == 0, counts
            print('PASS ten portal sessions closed after consent errors, active EIS errors, consent cancellation, and CreateSession cancellation; zero live sessions; no desktop input')
        except BaseException:
            for name in ['portal.log', 'daemon.log']:
                path = root / name
                if path.exists():
                    print(name + ':\n' + '\n'.join(path.read_text(errors='replace').splitlines()[-12:]), file=sys.stderr)
            raise
        finally:
            helpers.stop_process(daemon)
            helpers.stop_process(portal)
            if daemon_log:
                daemon_log.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--mock', type=Path)
    parser.add_argument('--inside', type=Path)
    args = parser.parse_args()
    if args.mock:
        asyncio.run(mock(args.mock))
    elif args.inside:
        inside(args.inside)
    else:
        with tempfile.TemporaryDirectory(prefix='simple-stt-portal-sessions-') as folder:
            subprocess.run(['dbus-run-session', '--', sys.executable, __file__, '--inside', folder], check=True, timeout=90)


if __name__ == '__main__':
    main()
