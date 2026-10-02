const assert = require("node:assert/strict");
const path = require("node:path");
const fs = require("node:fs");
const os = require("node:os");
const readline = require("node:readline");
const { spawn } = require("node:child_process");
const { chromium } = require("playwright");
(async () => {
  const { assertConfig } = await import("../web/settings/src/lib/types.ts");
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), "simple-stt-contracts-"));
  const binary = path.resolve(
    "target/debug/simple-stt-settings" +
      (process.platform === "win32" ? ".exe" : ""),
  );
  const child = spawn(binary, ["--no-browser"], {
    cwd: temp,
    env: {
      ...process.env,
      SIMPLE_STT_CONFIG: path.join(temp, "config.json"),
      SIMPLE_STT_RUNTIME_ROOT: path.join(temp, "runtime"),
      XDG_DATA_HOME: path.join(temp, "data"),
    },
  });
  let browser;
  try {
    const url = await new Promise((resolve, reject) => {
      readline.createInterface({ input: child.stdout }).once("line", resolve);
      child.once("exit", () =>
        reject(Error("Settings exited before starting")),
      );
    });
    const parsed = new URL(url);
    const headers = {
      "X-Simple-STT-Token": new URLSearchParams(parsed.hash.slice(1)).get(
        "token",
      ),
    };
    const state = await (
      await fetch(parsed.origin + "/api/state", { headers })
    ).json();
    assertConfig(state.config);
    assertConfig(
      (await (await fetch(parsed.origin + "/api/defaults", { headers })).json())
        .config,
    );
    const normalized = await (
      await fetch(parsed.origin + "/api/normalize", {
        method: "POST",
        headers: { ...headers, "Content-Type": "application/json" },
        body: JSON.stringify({ schema_version: 8 }),
      })
    ).json();
    assertConfig(normalized.config);
    const discovery = await (
      await fetch(parsed.origin + "/api/keyboard-languages", { headers })
    ).json();
    assert.equal(typeof discovery.available, "boolean");
    assert(Array.isArray(discovery.languages));
    for (const l of discovery.languages) {
      assert.equal(typeof l.id, "string");
      assert.equal(typeof l.name, "string");
    }
    for (const model of state.models) {
      assert.equal(typeof model.file, "string");
      assert.equal(typeof model.installed, "boolean");
      assert(Array.isArray(model.languages));
    }
    const invalid = structuredClone(state.config);
    invalid.speech.single_model_filename = 123;
    assert.throws(() => assertConfig(invalid));
    invalid.speech.single_model_filename = null;
    invalid.speech.language_models = [];
    assert.throws(() => assertConfig(invalid));
    browser = await chromium.launch({ headless: true });
    const page = await browser.newPage();
    const errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    const response = await page.goto(url);
    assert.match(
      response.headers()["content-security-policy"],
      /script-src 'self'/,
    );
    await page.locator('[data-setting-path="general.enabled"] input').waitFor();
    await page.locator('nav button[data-page="audio"]').click();
    await page
      .locator('[data-setting-path="speech.selection_mode"] select')
      .waitFor();
    assert.deepEqual(errors, [], "compiled UI must run under real Rust CSP");
    fs.writeFileSync(path.join(temp, "config.json"), "{invalid");
    await page.reload();
    await page.locator('[data-setting-path="general.enabled"] input').waitFor();
    await page.locator("#savebar button[type=submit]").click();
    await page
      .getByText("Saved. Capture service is offline.", { exact: true })
      .waitFor();
    assert.equal(
      JSON.parse(fs.readFileSync(path.join(temp, "config.json")))
        .schema_version,
      9,
    );
    assertConfig(
      (await (await fetch(parsed.origin + "/api/state", { headers })).json())
        .config,
    );
    await fetch(parsed.origin + "/api/close", {
      method: "POST",
      headers,
      body: "{}",
    });
    console.log(
      "PASS TypeScript contracts against real Rust state/defaults/migration/discovery/catalog and compiled browser UI under CSP",
    );
  } finally {
    await browser?.close();
    child.kill();
    fs.rmSync(temp, { recursive: true, force: true });
  }
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
