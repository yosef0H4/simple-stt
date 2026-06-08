# IPC proof of concept

This retained dependency-free prototype mirrors the selected shell control design without loading audio or Parakeet. `mock_service.py` binds only `127.0.0.1`, writes a discovery state file, requires a per-launch token, accepts a protocol-v1 handshake, answers `PING`, records `START_RECORDING`, emits an asynchronous Unicode `TRANSCRIPT` after `STOP_RECORDING`, and supports polling events after a sequence number.

The production implementation is in `src/capture/ipc_server.rs`, `src/bin/uvoxctl.rs`, and `ahk/lib/IpcClient.ahk`. The AHK client launches one-shot helpers asynchronously and polls process completion with `SetTimer()`, so hotkeys and GUI callbacks never wait on a socket read.

Run:

```powershell
python tools\ipc-poc\test_poc.py
```

Observed in the editing environment on 2026-06-07:

```text
IPC POC PASSED
 - authenticated loopback PING/PONG
 - START_RECORDING / STOP_RECORDING
 - asynchronous polled Unicode TRANSCRIPT
 - state-file reconnect after simulated service restart
```

This validates the transport pattern and reconnect design without building Rust or running AutoHotkey. Windows integration remains a target-machine validation step.
