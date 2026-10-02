import { createHash } from "node:crypto";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { gzipSync } from "node:zlib";
const hash = (p) => createHash("sha256").update(readFileSync(p)).digest("hex");
const walk = (dir) =>
  readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? walk(`${dir}/${e.name}`) : [`${dir}/${e.name}`],
  );
const files = [
  ...walk("src"),
  ...walk("tools"),
  "index.html",
  "package.json",
  "package-lock.json",
  "vite.config.ts",
  "tsconfig.json",
  "svelte.config.js",
].sort();
const assets = ["index.html", "app.js", "styles.css"];
const manifest = {
  sources: Object.fromEntries(files.map((p) => [p, hash(p)])),
  assets: Object.fromEntries(assets.map((p) => [p, hash(`dist/${p}`)])),
};
const gzip = assets
  .filter((p) => p.endsWith(".js") || p.endsWith(".css"))
  .reduce((n, p) => n + gzipSync(readFileSync(`dist/${p}`)).length, 0);
const total = assets.reduce((n, p) => n + readFileSync(`dist/${p}`).length, 0);
if (gzip > 100 * 1024 || total > 300 * 1024)
  throw Error(`Asset budget exceeded: ${gzip} gzip bytes, ${total} raw bytes`);
const content = JSON.stringify(manifest, null, 2) + "\n";
if (process.argv.includes("--verify")) {
  if (readFileSync("dist/manifest.json", "utf8") !== content)
    throw Error("Frontend assets are stale; run npm run build");
} else writeFileSync("dist/manifest.json", content);
console.log(
  `Frontend assets verified: ${(gzip / 1024).toFixed(1)} KiB gzip, ${(total / 1024).toFixed(1)} KiB raw`,
);
