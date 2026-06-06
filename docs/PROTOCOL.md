# IPC protocol

Rust listens on `127.0.0.1` using an operating-system-selected port and spawns Python with the callback address plus a random token.

## Frame header

Every frame starts with:

| Offset | Size | Meaning |
|---:|---:|---|
| `0` | `1` | frame kind |
| `1` | `4` | payload byte length, little-endian unsigned integer |
| `5` | variable | payload |

Maximum payload size is 4 MiB.

## Frame kinds

| Kind | Payload |
|---:|---|
| `1` | UTF-8 JSON object |
| `2` | eight-byte little-endian session ID followed by little-endian signed PCM16 samples |

## Handshake

Python immediately sends:

```json
{"type":"hello","token":"<random-token>","protocol":1}
```

Rust rejects mismatched tokens or protocol versions.

## Commands from Rust

```json
{"type":"start","session_id":1}
{"type":"cancel","session_id":1}
{"type":"shutdown"}
{"type":"ping"}
```

Audio travels in binary PCM16 frames rather than JSON.

## Events from Python

```json
{"type":"status","state":"loading_model"}
{"type":"status","state":"ready"}
{"type":"session_started","session_id":1}
{"type":"partial","session_id":1,"text":"revisable hypothesis"}
{"type":"commit","session_id":1,"text":"stable prefix delta "}
{"type":"session_cancelled","session_id":1}
```
