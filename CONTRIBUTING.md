# Contributing

Read `AGENTS.md` first.

Keep changes narrow and testable. Add or update unit tests when changing protocol, cancellation, resampling, config, or stabilization behavior.

Run:

```powershell
.\scripts\test-all.ps1
```

When changing the real model path, also run:

```powershell
.\scripts\first-test.ps1
```

Do not introduce CPU fallback, browser-based GUI frameworks, random anti-detection timing, or hidden behavior intended to bypass target application restrictions.
