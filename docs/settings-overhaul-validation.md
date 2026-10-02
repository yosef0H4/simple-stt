# Settings overhaul validation

Measured on Linux on 2026-10-02 against release `3dba987` using identical local
HTTP fixtures, Chromium, viewports, configurations, and workloads. These are
local measurements, not universal performance guarantees.

| Metric | Baseline | Svelte/TypeScript | Result |
| --- | ---: | ---: | --- |
| Median Settings load | 8.17 ms | 7.17 ms | -12.2%, below 10% limit |
| Median page interaction | 31.1 ms | 31.2 ms | +0.3%, below 10% limit |
| Settled tab JavaScript heap | 2,415,900 bytes | 3,294,192 bytes | +878,292 bytes, below 5 MiB limit |
| Median release Settings-server RSS | 8,335,360 bytes | 8,318,976 bytes | -16,384 bytes, below 2 MiB allowance |
| Compiled JS/CSS gzip | — | 38,556 bytes | below 100 KiB limit |
| Served frontend assets | — | 114,746 bytes | below 300 KiB limit |

Browser measurements use 12 loads, ten warm navigation cycles, then forty
measured cycles. Each cycle visits every page, uses searchable dropdowns, and
processes download progress/completion events. Forced garbage collection samples
on the new UI were 3,201,468, 3,285,776, 3,318,988, and 3,294,192 bytes; retained
heap settled. The final page had 178 DOM nodes versus 755 in the original UI.
Frame-bound interaction timing includes two animation frames on both versions.

RSS uses five isolated release processes per version, loading the same assets
and API responses, then closing each process through the authenticated API.
The existing Linux shell/capture PIDs remained running throughout; neither
resident code nor process was replaced. Their sampled RSS did not increase.
Browser shared-engine RAM is separate from this tab's JavaScript heap.

Checks passed:

- Strict Svelte/TypeScript checks, production build, fingerprint and size checks.
- Cargo formatting and Clippy with all targets/features; 114 Rust tests.
- Real Rust Settings API selection and cleanup process tests; static and IPC checks.
- Three browser suites: model selection, all Settings pages, and real Rust API/CSP contracts.
- Save/Reset/import/export, JSON draft persistence and malformed-file recovery,
  conflict protection, offline saves, shortcuts, cleanup actions and cancelled page requests.
- Light/dark layouts at 360/768/1280 pixels, 200% zoom, keyboard dropdowns,
  unavailable/None choices, Arabic text, long filenames, System theme, focus tooltips,
  and one batched visual review followed by confirmation.
- Stale source rejection by Cargo and Python, including newly added unbuilt sources.
- Windows GNU cross-build of all binaries and Windows-target Clippy.

Native Windows/AHK desktop runtime tests remain unvalidated because no Windows
host was available. Keyboard routing and Vulkan inference code were unchanged
by this UI overhaul; the earlier real Linux English/Arabic/GPU validations apply
to that behavior.

Reproduce with the browser/RSS commands in `docs/testing.md`. Baseline assets
and local reports are ignored artifacts, not shipped runtime dependencies.
