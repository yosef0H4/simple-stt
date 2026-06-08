# Skill: debug the Uvox pipeline

Use the component boundaries in order:

```powershell
.\scripts\test-static.ps1
cargo test --all-targets
.\scripts\check-prereqs.ps1 -RequireRuntime
.\scripts\run-dev.ps1 -SkipBuild
```

Expected live sequence:

```text
AHK hotkey down
→ uvoxctl start-recording
→ capture retains 20 ms PCM frames
→ AHK hotkey up
→ uvoxctl stop-recording
→ capture launches or reuses uvox-infer
→ worker loads model lazily when cold
→ worker returns Unicode transcript
→ AHK event poll queues focus-checked SendText chunks
→ idle timeout terminates uvox-infer process
```

Inspect separate logs under `%LOCALAPPDATA%\uvox\logs`. Never “fix” capture failures by loading Parakeet inside `uvox-capture.exe`.
