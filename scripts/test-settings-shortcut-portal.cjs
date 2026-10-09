const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const net = require("node:net");
const { spawn } = require("node:child_process");
const { chromium } = require("playwright");
const f = require("./settings-fixture.cjs");

const freePort = () =>
  new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close((error) => (error ? reject(error) : resolve(port)));
    });
  });

(async () => {
  let browser;
  let vite;
  try {
    f.options.platform = "linux";
    f.options.systemLocale = "en";
    await new Promise((resolve) => f.server.listen(0, "127.0.0.1", resolve));

    const port = await freePort();
    vite = spawn(
      process.execPath,
      [
        path.resolve("web/settings/node_modules/vite/bin/vite.js"),
        "--host",
        "127.0.0.1",
        "--port",
        String(port),
        "--strictPort",
      ],
      {
        cwd: path.resolve("web/settings"),
        env: {
          ...process.env,
          SIMPLE_STT_SETTINGS_ORIGIN: `http://127.0.0.1:${f.server.address().port}`,
        },
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
    let viteOutput = "";
    vite.stdout.on("data", (chunk) => (viteOutput += chunk));
    vite.stderr.on("data", (chunk) => (viteOutput += chunk));
    const url = `http://127.0.0.1:${port}/#token=${f.token}`;
    await new Promise((resolve, reject) => {
      const timeout = setTimeout(
        () => reject(Error(`Vite did not start: ${viteOutput}`)),
        15000,
      );
      const check = () => {
        if (vite.exitCode !== null)
          return reject(Error(`Vite exited early: ${viteOutput}`));
        fetch(url.split("#")[0])
          .then((response) => {
            if (response.ok) {
              clearTimeout(timeout);
              resolve();
            } else setTimeout(check, 100);
          })
          .catch(() => setTimeout(check, 100));
      };
      check();
    });

    browser = await chromium.launch({
      headless: true,
      executablePath: process.env.CHROMIUM_PATH || undefined,
    });
    const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await page.goto(url);
    await page.locator('[data-setting-path="general.enabled"] input').waitFor();
    const language = page.locator(
      '[data-setting-path="general.ui_language"] select',
    );
    await language.selectOption("ar");
    await page.waitForFunction(() => document.documentElement.dir === "rtl");

    const setup = page.getByRole("button", {
      name: "إعداد الاختصارات",
      exact: true,
    });
    await setup.waitFor();
    const bounds = await setup.boundingBox();
    assert(bounds && bounds.width > 80, "shortcut setup button must show its label");
    await setup.click();
    assert(
      f.actions.some((action) => action.action === "configure_shortcuts"),
      "the visible button must call configure_shortcuts",
    );
    await page.waitForFunction(() => {
      const button = [...document.querySelectorAll("button")].find((item) =>
        item.textContent.includes("إعداد الاختصارات"),
      );
      return button && !button.disabled;
    });

    const artifacts = path.resolve("artifacts/settings-shortcut-portal");
    fs.mkdirSync(artifacts, { recursive: true });
    for (const locale of ["en", "ar"]) {
      await language.selectOption(locale);
      const actionButton = page.getByRole("button", { name: locale === "en" ? "Set up shortcuts" : "إعداد الاختصارات", exact: true });
    for (const theme of ["light", "dark"]) {
      await page
        .locator('[data-setting-path="general.ui_theme"] select')
        .selectOption(theme);
      for (const width of [360, 1280]) {
        await page.setViewportSize({ width, height: 900 });
        await page.screenshot({
          path: path.join(artifacts, `${locale}-${theme}-${width}.png`),
          fullPage: true,
        });
        const buttonBounds = await actionButton.boundingBox();
        const headerBounds = await page.locator(".group-head").first().boundingBox();
        const scrollX = await page.evaluate(() => window.scrollX);
        assert(buttonBounds && headerBounds, "shortcut action and group heading should remain visible");
        assert(
          buttonBounds.x >= headerBounds.x &&
            buttonBounds.x + buttonBounds.width <= headerBounds.x + headerBounds.width,
          `shortcut button should fit its header at ${width}px in ${theme} mode: ${JSON.stringify({ buttonBounds, headerBounds, scrollX })}`,
        );
        await page.screenshot({
          path: path.join(artifacts, `${locale}-${theme}-${width}.png`),
          fullPage: true,
        });
      }
    }

    }
    await language.selectOption("ar");
    let failureIntercepted = false;
    await page.route("**/api/platform-action", (route) => {
      failureIntercepted = true;
      return route.fulfill({
        status: 500,
        contentType: "application/json",
        body: JSON.stringify({ error: "Portal dialog is unavailable for this test" }),
      });
    });
    const failureResponse = page.waitForResponse(
      (response) =>
        response.url().includes("/api/platform-action") && response.status() === 500,
    );
    await setup.click();
    await failureResponse;
    assert(failureIntercepted, "simulated platform-action failure should be intercepted");
    await page.locator('#notice[data-kind="error"]').waitFor();
    await page
      .locator(".notice-detail [dir=auto]")
      .getByText("Portal dialog is unavailable for this test", { exact: true })
      .waitFor();
    await page.unroute("**/api/platform-action");
    assert.deepEqual(errors, [], errors.join("\n"));
    console.log(
      "PASS visible shortcut setup action, configure_shortcuts request, actionable error, Arabic RTL layout at 360/1280 in light/dark themes",
    );
  } finally {
    await browser?.close();
    vite?.kill();
    if (f.server.listening)
      await new Promise((resolve) => f.server.close(resolve));
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
