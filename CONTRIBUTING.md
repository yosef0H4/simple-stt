# Contributing

Read `AGENTS.md` first.

Keep changes narrow and testable. Add or update unit tests when changing config, resampling, text insertion, hotkey state, or the native Parakeet FFI boundary.

Run:

```powershell
.\scripts\check-prereqs.ps1
.\scripts\test-audio.ps1
cargo test -p uvox
```

For app behavior changes, also manually run:

```powershell
.\scripts\run.ps1
```

Do not introduce CPU fallback, Python/C# helper processes, browser-based GUI frameworks, random anti-detection timing, or hidden behavior intended to bypass target application restrictions.
