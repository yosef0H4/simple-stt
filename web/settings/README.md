# Settings frontend

Svelte components and TypeScript source live in `src/`. Only `dist/index.html`,
`dist/app.js`, and `dist/styles.css` are embedded in the Rust Settings executable.
Node, npm, the development server, and Impeccable are never packaged.

```sh
cd web/settings
npm ci
npm run check
npm run build
npm run verify
```

Commit the source, npm lockfile, and generated `dist/` assets together. Cargo
checks their source fingerprints and refuses stale output. Unchanged checkouts
can still build with Cargo alone, including offline and Windows cross-builds.
The generator enforces 100 KiB gzip for combined JS/CSS and 300 KiB total assets.

To develop interactively, start an isolated `simple-stt-settings --no-browser`
server, then set `SIMPLE_STT_SETTINGS_ORIGIN` to its printed origin and run
`npm run dev`. Open Vite's loopback URL with the server's `#token=...` fragment.
The development proxy forwards only `/api` to that server; use a temporary
configuration when testing writes. Never commit tokens or credentials.

Shared controls live in `src/components/`; page components in `src/pages/`.
`lib/types.ts` defines the wire contracts, `api.ts` handles authentication,
`state.svelte.ts` owns drafts/events, and `settings.ts` indexes settings search.
The active page mounts alone. All page-independent state lives in the draft;
page-local effects and global event requests are cancelled on destruction.

Browser tests use the deterministic local fixture in `scripts/settings-fixture.cjs`.
Run the browser suites with Playwright available through `NODE_PATH` or a local
installation. `scripts/benchmark-settings.cjs` measures release assets using
separate browser-tab JS heap metrics. Set `SETTINGS_WEB_ROOT` for a baseline and
`SETTINGS_BENCHMARK_OUTPUT` for a JSON report.
