const assert = require("node:assert/strict");
const { chromium } = require("playwright");
const fs = require("node:fs");
const path = require("node:path");
const f = require("./settings-fixture.cjs");
(async () => {
  await new Promise((r) => f.server.listen(0, "127.0.0.1", r));
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage({
      viewport: { width: 1280, height: 900 },
    });
    const client = await page.context().newCDPSession(page);
    const url = `http://127.0.0.1:${f.server.address().port}/#token=${f.token}`;
    const starts = [],
      interactions = [];
    for (let i = 0; i < 12; i++) {
      const begin = performance.now();
      await page.goto(url);
      await page
        .locator('[data-setting-path="general.enabled"] input')
        .waitFor();
      starts.push(performance.now() - begin);
    }
    const heap = async () => {
      await client.send("HeapProfiler.collectGarbage");
      return (await client.send("Runtime.getHeapUsage")).usedSize;
    };
    async function navigate(measure) {
      for (const name of [
        "audio",
        "models",
        "output",
        "cleanup",
        "advanced",
        "config",
        "general",
      ]) {
        const ms = await page.evaluate(async (name) => {
          const begin = performance.now();
          document.querySelector(`nav button[data-page="${name}"]`).click();
          await new Promise((r) =>
            requestAnimationFrame(() => requestAnimationFrame(r)),
          );
          return performance.now() - begin;
        }, name);
        if (measure) interactions.push(ms);
        if (name === "audio") {
          for (const combo of await page.getByRole("combobox").all()) {
            if ((await combo.getAttribute("role")) !== "combobox") continue;
            await combo.focus();
            await combo.press("ArrowDown");
            await combo.press("Escape");
            await combo.blur();
          }
        }
      }
      // Progress events arrive even while another page is mounted.
      f.publishEvent("model_download_progress", {
        filename: "english-new-q8.gguf",
        downloaded: "50",
        total: "100",
      });
      const refreshed = page.waitForResponse((r) =>
        r.url().endsWith("/api/state"),
      );
      f.publishEvent("model_download_complete", {
        filename: "english-new-q8.gguf",
      });
      await refreshed;
      await page.evaluate(
        () =>
          new Promise((r) =>
            requestAnimationFrame(() => requestAnimationFrame(r)),
          ),
      );
      assert(f.eventWaiters.size <= 1, "Only one event subscription may exist");
    }
    // Warm every page and dropdown before testing sustained retained growth.
    for (let i = 0; i < 10; i++) await navigate(false);
    const before = await heap(),
      samples = [];
    for (let i = 0; i < 40; i++) {
      await navigate(true);
      if ((i + 1) % 10 === 0) samples.push(await heap());
    }
    const after = samples.at(-1);
    const median = (a) =>
      [...a].sort((x, y) => x - y)[Math.floor(a.length / 2)];
    const report = {
      startup_ms: median(starts.slice(2)),
      interaction_ms: median(interactions),
      heap_before: before,
      heap_after: after,
      heap_growth: after - before,
      heap_samples: samples,
      dom_nodes: await page.locator("*").count(),
    };
    if (process.env.SETTINGS_BENCHMARK_BASELINE) {
      const baseline = JSON.parse(
        fs.readFileSync(process.env.SETTINGS_BENCHMARK_BASELINE),
      );
      report.startup_ratio = report.startup_ms / baseline.startup_ms;
      report.interaction_ratio =
        report.interaction_ms / baseline.interaction_ms;
      report.heap_delta = report.heap_after - baseline.heap_after;
      assert(
        report.startup_ratio <= 1.1,
        "Startup median exceeds 10% slowdown budget",
      );
      assert(
        report.interaction_ratio <= 1.1,
        "Interaction median exceeds 10% slowdown budget",
      );
      assert(
        report.heap_delta <= 5 * 1024 * 1024,
        "Settled heap exceeds 5 MiB delta",
      );
      assert(
        samples.at(-1) - samples[0] <= 256 * 1024,
        "Sustained retained heap growth",
      );
    }
    console.log(JSON.stringify(report, null, 2));
    if (process.env.SETTINGS_BENCHMARK_OUTPUT) {
      fs.mkdirSync(path.dirname(process.env.SETTINGS_BENCHMARK_OUTPUT), {
        recursive: true,
      });
      fs.writeFileSync(
        process.env.SETTINGS_BENCHMARK_OUTPUT,
        JSON.stringify(report, null, 2),
      );
    }
  } finally {
    await browser.close();
    f.closeEvents();
    await new Promise((r) => f.server.close(r));
  }
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
