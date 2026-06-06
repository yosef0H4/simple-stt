# Protocol

Uvox no longer uses a worker IPC protocol.

The current product path is native and in-process:

```text
Rust audio buffer → parakeet.dll C API → transcript string → Win32 SendInput
```

Historical Python worker and loopback TCP protocol code was removed during the native Parakeet overhaul.
