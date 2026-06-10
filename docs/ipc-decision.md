# IPC decision and proof of concept

## Requirements

The shell-to-capture control path must be local-only, Unicode-safe, reconnectable after a service restart, protocol-versioned, token-authenticated, easy to log, tolerant of malformed input, and non-blocking inside AHK hotkey/GUI callbacks. AHK must never receive raw PCM.

## Compared designs

### 1. Windows named pipes

Named pipes are a native Windows IPC mechanism and can be used across processes. Microsoft documents that named pipes can also be used across a network depending on configuration and access controls, so a safe local-only design needs deliberate ACL and remote-client handling.

Pros:

- native local stream transport;
- no listening TCP socket;
- suitable for Rust-to-Rust or native clients.

Cons for the AHK boundary:

- direct pipe opens, reconnect, timeout handling, overlapped reads, and ACL details increase the shell's Win32 surface;
- blocking `FileOpen()` / pipe reads would be risky in hotkey or GUI callbacks;
- asynchronous pipe handling would require more `DllCall()` plumbing than the thin-shell goal allows.

Microsoft sources:

- <https://learn.microsoft.com/windows/win32/ipc/named-pipes>
- <https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-createnamedpipea>

Decision: not selected for AHK-to-capture. Anonymous child pipes are selected for Rust capture-to-infer IPC.

### 2. `WM_COPYDATA` with AHK `OnMessage()`

Microsoft documents `WM_COPYDATA` as a way to pass data to another application. The receiving side must treat the payload as valid only during message processing. `SendMessage()` does not return until the receiving window procedure processes the message.

Pros:

- built into Windows GUI messaging;
- AHK has `OnMessage()`.

Cons:

- synchronous request delivery is a poor fit for long-lived asynchronous transcript events;
- window discovery and restart reconnect add complexity;
- payload lifetime constraints must be handled carefully;
- it invites too much logic into an AHK message callback.

Microsoft sources:

- <https://learn.microsoft.com/windows/win32/dataxchg/wm-copydata>
- <https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-sendmessage>

Decision: rejected.

### 3. Disposable `simple-stt-ctl.exe` helper with loopback service control socket

The AHK shell launches a one-shot helper asynchronously with `Run()`, records its PID, and checks completion from `SetTimer()`. The helper opens the capture service state file, connects to an address bound explicitly to `127.0.0.1`, performs the version/token handshake, issues one JSON command, writes a tiny escaped UTF-8 response file, and exits.

Pros:

- AHK has no blocking socket or pipe read;
- no clipboard usage;
- JSON parsing and malformed-message rejection stay in Rust;
- helper process boundaries make stuck control requests disposable;
- reconnect is simple: each helper rereads the state file;
- a per-shell random token prevents accidental commands from unrelated local processes;
- `127.0.0.1:0` creates an ephemeral loopback listener with low operational complexity.

Cons:

- one small process launch per control request and event poll;
- state file and response-file cleanup need logging;
- the token is an accidental-command barrier, not a defense against another process running as the same user with access to the shell command line or process state.

Decision: selected for AHK-to-capture IPC.

## Production shell protocol

The control socket uses line-delimited JSON and an explicit handshake:

```json
{"type":"hello","protocol":1,"token":"<per-launch token>"}
{"type":"hello_ack","protocol":1,"service_pid":1234}
{"type":"command","request_id":7,"command":{"name":"ping"}}
{"type":"response","request_id":7,"response":{"ok":true,"message":"pong","values":{},"events":[]}}
```

Supported commands include ping, start/stop recording, event polling, config reload, model unload, model test, model download, microphone/model enumeration, overlay notice, and graceful shutdown. Malformed JSON, wrong handshake order, incompatible versions, and bad tokens return structured errors and are logged.

Rust source:

```text
src/common/shell_protocol.rs
src/capture/ipc_server.rs
src/bin/simple-stt-ctl.rs
ahk/lib/IpcClient.ahk
```

The AHK-readable helper response is an escaped UTF-8 tab protocol. It is intentionally tiny, licensed with this repository, and tested round-trip in `src/common/line_codec.rs`.

## Production Rust capture-to-infer protocol

The service spawns `simple-stt-infer.exe` lazily with piped stdin/stdout. Logs go to files and stderr; stdout is protocol-only. Frames have a fixed 20-byte header plus body:

```text
magic       4 bytes  "UVX1"
version     u16
message     u16
session id  u64
body length u32
body        bytes
```

PCM request bodies contain:

```text
sample rate  u32
sample count u32
PCM16 LE     2 * sample count bytes
```

This is implemented in `src/infer/protocol.rs`. The shell never sees PCM.

## Retained proof of concept

`tools/ipc-poc/` contains a dependency-free Python mock capture service and test. The test performs:

1. state-file publication;
2. authenticated protocol-v1 handshake;
3. `PING` / `PONG`;
4. `START_RECORDING`;
5. `STOP_RECORDING`;
6. asynchronous Unicode transcript polling with `مرحبا 世界 🙂`;
7. simulated service exit;
8. state-file re-publication with a new token;
9. reconnect and a second `PING`.

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

This proves the transport pattern independently of Rust compilation and Windows-only AHK execution. Windows integration remains a target-machine validation step.
