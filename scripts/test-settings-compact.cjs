const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { chromium } = require("playwright");
const fixture = require("./settings-fixture.cjs");

async function main() {
  fixture.options.platform = "windows";
  Object.assign(fixture.getConfig().general, {
    ui_theme: "dark",
    ui_language: "en",
    show_shortcuts_hotkey: "CapsLock",
    show_shortcuts_mode: "hold",
  });
  await new Promise((resolve) => fixture.server.listen(0, "127.0.0.1", resolve));
  const browser = await chromium.launch();
  const screenshots = path.resolve("artifacts/compact-settings");
  fs.mkdirSync(screenshots, { recursive: true });
  try {
    const page = await browser.newPage({ viewport: { width: 891, height: 661 } });
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    const url = new URL("http://127.0.0.1:" + fixture.server.address().port);
    url.hash = "token=" + fixture.token;
    await page.goto(url.href);
    const main = page.locator(".app-shell > main");
    const rail = page.locator(".rail");
    const dictation = page.locator("#shortcuts-dictation-heading");
    const go = (section) => page.locator('nav button[data-page="' + section + '"]').click();
    await dictation.waitFor();
    await page.evaluate(() => document.fonts.ready);
    assert.equal((await rail.boundingBox()).width, 184);
    assert((await dictation.boundingBox()).y < 250, "Dictation should appear beside App-wide shortcuts");
    const record = page.locator('[data-setting-path="general.record_hotkey"] input');
    const recordWidth = await record.evaluate((input) => {
      const style = getComputedStyle(input);
      const context = document.createElement("canvas").getContext("2d");
      context.font = style.font;
      const contentWidth = input.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
      return contentWidth - context.measureText(input.value).width;
    });
    assert(recordWidth >= 0, "The default recording chord should fit the input");
    await page.screenshot({ path: path.join(screenshots, "compact.png"), animations: "disabled" });

    const toggle = page.locator('[data-setting-path="general.enabled"] input');
    await toggle.uncheck();
    fixture.publishEvent("notice", {}, "A Settings status remains below the panel.");
    const status = page.locator("#notice");
    await status.getByText("A Settings status remains below the panel.").waitFor();
    const transcriptSeq = fixture.publishEvent("transcript", {}, "Private dictation output");
    await page.waitForRequest((request) => {
      const address = new URL(request.url());
      return address.pathname === "/api/events" && address.searchParams.get("after") === String(transcriptSeq);
    });
    assert.equal(await status.innerText(), "A Settings status remains below the panel.");
    const feedback = await page.locator(".settings-feedback").boundingBox();
    const body = await main.boundingBox();
    assert(body.y + body.height <= feedback.y + 1, "Feedback must have its own layout space");
    await page.locator("#savebar button[type=submit]").click();
    await page.locator("#savebar").waitFor({ state: "detached" });

    for (const width of [1024, 680, 360, 320]) {
      await page.setViewportSize({ width, height: 600 });
      for (const section of ["general", "audio", "reading", "models", "cleanup", "advanced"]) {
        await go(section);
        await page.locator('[data-section="' + section + '"]').waitFor();
        assert(await main.evaluate((pane) => pane.scrollWidth <= pane.clientWidth + 1),
          section + " overflows the content pane at " + width);
      }
    }
    await go("general");
    assert((await rail.boundingBox()).height < 150, "Narrow navigation should leave most of the window for settings");
    await page.locator("#settings-search").fill("Recording mode");
    await page.getByRole("option", { name: /Recording mode/i }).click();
    const recording = page.locator('[data-setting-path="general.recording_mode"] input');
    await recording.waitFor();
    assert(await recording.evaluate((element) => {
      const view = element.closest("main").getBoundingClientRect();
      const field = element.getBoundingClientRect();
      return field.top >= view.top && field.bottom <= view.bottom;
    }), "Search must scroll the content pane to the selected control");

    await page.setViewportSize({ width: 891, height: 661 });
    await go("general");
    await main.evaluate((pane) => { pane.scrollTop = 0; });
    assert.equal(await page.locator(".page-head").evaluate((header) => getComputedStyle(header).borderTopWidth), "0px");
    assert.equal(await page.locator(".app-shell").evaluate((shell) => getComputedStyle(shell).backgroundImage), "none");
    assert.equal(fixture.getConfig().general.ui_theme, "dark");
    await page.screenshot({ path: path.join(screenshots, "classic.png"), animations: "disabled" });
    await go("models");
    const models = page.locator('[data-download-kind="model"]');
    const runtimes = page.locator('[data-download-kind="runtime"]');
    const bevel = (button) => button.evaluate((element) => getComputedStyle(element).boxShadow);
    const raised = await bevel(runtimes);
    const sunken = await bevel(models);
    assert.notEqual(raised, sunken);
    await runtimes.hover();
    assert.equal(await bevel(runtimes), raised, "Hover leaves the unselected button raised");
    await page.mouse.down();
    assert.equal(await bevel(runtimes), sunken, "Holding the pointer down presses the button");
    await page.mouse.up();
    await page.mouse.move(1, 1);
    assert.equal(await runtimes.getAttribute("aria-pressed"), "true");
    assert.equal(await bevel(runtimes), sunken, "The selected menu stays pressed after release");
    assert.equal(await bevel(models), raised, "The previous selection returns to raised");
    await models.click();
    await page.screenshot({ path: path.join(screenshots, "classic-models.png"), animations: "disabled" });
    await go("audio");
    const slider = page.locator("#typing-speed");
    await slider.scrollIntoViewIfNeeded();
    await slider.press("End");
    const fill = () => slider.evaluate((element) => getComputedStyle(element).getPropertyValue("--range-fill").trim());
    assert.equal(await fill(), "100%");
    await slider.press("Home");
    assert.equal(await fill(), "0%");
    await slider.press("ArrowRight");
    assert(parseFloat(await fill()) > 0);
    await go("advanced");
    const resetMemory = page.locator(".group-head .icon-button").first();
    assert.equal(await bevel(resetMemory), raised, "The memory reset has a raised edge before hover");
    await resetMemory.hover();
    assert.equal(await bevel(resetMemory), raised, "Hover leaves the reset button raised");
    await page.mouse.down();
    assert.equal(await bevel(resetMemory), sunken, "The reset button presses down on click");
    await page.mouse.move(1, 1);
    await page.mouse.up();
    for (const id of ["reading-idle", "ocr-idle"]) {
      const idle = page.locator(`#${id}`);
      const output = page.locator(`output[for="${id}"]`);
      const idleFill = () => idle.evaluate((element) => element.style.getPropertyValue("--range-fill"));
      assert.equal(await idle.evaluate((element) => getComputedStyle(element).appearance), "none");
      await idle.press("End");
      assert.equal(await idleFill(), "100%");
      assert.equal(await output.textContent(), "3600s");
      await idle.press("Home");
      assert.equal(await idleFill(), "0%");
      assert.equal(await output.textContent(), "0s");
      await idle.fill("300");
    }
    await main.evaluate((pane) => { pane.scrollTop = 0; });
    await page.screenshot({ path: path.join(screenshots, "classic-system.png"), animations: "disabled" });
    assert.equal(await page.locator('#classic-preview, [data-setting-path="appearance.classic"]').count(), 0);
    assert.deepEqual(errors, []);
    console.log("PASS compact Settings: dense tiles, reflow, search, Classic buttons and segmented sliders");
  } finally {
    await browser.close();
    fixture.closeEvents();
    await new Promise((resolve) => fixture.server.close(resolve));
  }
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
