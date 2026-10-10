const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { chromium } = require("playwright");
const f = require("./settings-fixture.cjs");

(async () => {
  await new Promise((resolve) => f.server.listen(0, "127.0.0.1", resolve));
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage();
    const url = `http://127.0.0.1:${f.server.address().port}/#token=${f.token}`;
    await page.goto(url);
    await page.locator('[data-setting-path="general.enabled"] input').waitFor();

    assert.equal(f.getConfig().diagnostics.clipboard_buffer_notices, false);
    await page.locator('nav button[data-page="advanced"]').click();
    await page.locator('section[data-section="advanced"]').waitFor();
    assert.equal(
      await page.locator('[data-setting-path="diagnostics.clipboard_buffer_notices"]').count(),
      0,
      "the clipboard notice setting is hidden on Windows",
    );
    await page.locator("#settings-search").fill("Clipboard buffer notices");
    assert.equal(await page.locator('[role="option"]').count(), 0);

    f.options.platform = "linux";
    await page.reload();
    await page.locator('[data-setting-path="general.enabled"] input').waitFor();
    await page.locator("#settings-search").fill("Clipboard buffer notices");
    await page
      .getByRole("option", { name: /Clipboard buffer notices/ })
      .waitFor();
    await page
      .getByRole("option", { name: /Clipboard buffer notices/ })
      .press("Enter");
    await page.locator('nav button[data-page="advanced"]').click();
    const notices = page.locator(
      '[data-setting-path="diagnostics.clipboard_buffer_notices"] input',
    );
    await notices.waitFor();
    assert.equal(
      await page
        .locator('[data-setting-path="diagnostics.clipboard_buffer_notices"] label')
        .innerText(),
      "Clipboard buffer notices",
    );
    const screenshotDir = path.resolve(
      __dirname,
      "../artifacts/linux-release/clipboard-buffer-notices",
    );
    fs.mkdirSync(screenshotDir, { recursive: true });
    await page.screenshot({
      path: path.join(screenshotDir, "advanced-en.png"),
      fullPage: true,
    });
    assert.equal(await notices.isChecked(), false);
    await notices.check();
    assert.equal(f.getConfig().diagnostics.clipboard_buffer_notices, false);
    await page.locator("#savebar button[type=submit]").click();
    await page.locator("#savebar").waitFor({ state: "detached" });
    assert.equal(f.getConfig().diagnostics.clipboard_buffer_notices, true);
    await page.reload();
    await page.locator('[data-setting-path="general.enabled"] input').waitFor();
    await page.locator('nav button[data-page="advanced"]').click();
    assert.equal(
      await page
        .locator('[data-setting-path="diagnostics.clipboard_buffer_notices"] input')
        .isChecked(),
      true,
      "the saved preference survives reload",
    );

    await page.locator('nav button[data-page="general"]').click();
    await page.locator("#ui-language").selectOption("ar");
    await page.locator("#savebar button[type=submit]").click();
    await page.locator("#savebar").waitFor({ state: "detached" });
    await page.locator("#settings-search").fill("إشعارات الحافظة");
    await page.getByRole("option", { name: /إشعارات الحافظة/ }).waitFor();
    await page.getByRole("option", { name: /إشعارات الحافظة/ }).press("Enter");
    await page
      .locator('[data-setting-path="diagnostics.clipboard_buffer_notices"] input')
      .waitFor();
    await page.screenshot({
      path: path.join(screenshotDir, "advanced-ar.png"),
      fullPage: true,
    });
    console.log("Clipboard buffer notices settings UI test passed");
  } finally {
    await browser.close();
    f.closeEvents();
    await new Promise((resolve) => f.server.close(resolve));
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
