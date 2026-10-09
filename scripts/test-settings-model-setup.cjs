const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require('playwright');
const f = require('./settings-fixture.cjs');
(async () => {
  let browser;
  try {
    await new Promise(resolve => f.server.listen(0, '127.0.0.1', resolve));
    browser = await chromium.launch({ headless: true });
    const page = await browser.newPage({ viewport: {width: 1280, height: 900} });
    let handled = false, failDownload = false, failDismiss = false, pending = false, installed = false;
    const requested = [], choices = [];
    await page.route('**/api/state', async route => {
      const response = await route.fetch();
      const state = await response.json();
      state.model_setup = { pending, show: !handled, english_file: 'english-q4.gguf', arabic_file: 'arabic-q8.gguf' };
      state.models.forEach(model => model.installed = installed);
      await route.fulfill({response, json: state});
    });
    await page.route('**/api/model-setup', async route => {
      if (failDismiss) return route.fulfill({status:400, json:{error:'Isolated setup persistence failure'}});
      const choice = route.request().postDataJSON();
      choices.push(choice);
      pending = choice.english || choice.arabic;
      handled = true;
      await route.fulfill({json:{ok:true}});
    });
    await page.route('**/api/action', async route => {
      const body = route.request().postDataJSON();
      if (body.action === 'download_model') {
        requested.push(body.filename);
        return route.fulfill({status: failDownload ? 400 : 200, json: failDownload ? {error:'Isolated download failure'} : {message:'Queued'}});
      }
      await route.continue();
    });
    const url = `http://127.0.0.1:${f.server.address().port}/#token=${f.token}`;
    const prompt = page.locator('#recommended-setup');
    await page.goto(url);
    await prompt.waitFor();
    const boxes = prompt.getByRole('checkbox');
    assert.equal(await boxes.count(), 2);
    assert(await boxes.nth(0).isChecked());
    assert(await boxes.nth(1).isChecked());
    assert.deepEqual(requested, [], 'first opening must not download automatically');
    const dir = path.resolve('artifacts/model-setup'); fs.mkdirSync(dir, {recursive:true});
    for (const language of ['en','ar']) {
      await page.locator('[data-setting-path="general.ui_language"] select').selectOption(language);
      for (const theme of ['light','dark']) {
        await page.locator('[data-setting-path="general.ui_theme"] select').selectOption(theme);
        for (const width of [360,768,1280]) {
          await page.setViewportSize({width,height:900});
          const box = await prompt.boundingBox();
          assert(box && box.x >= 0 && box.x + box.width <= width, 'prompt must fit the viewport');
          await page.screenshot({path:path.join(dir, `${language}-${theme}-${width}.png`), fullPage:true});
        }
      }
    }
    await page.locator('[data-setting-path="general.ui_language"] select').selectOption('en');
    await boxes.nth(0).uncheck();
    failDownload = true;
    await prompt.getByRole('button', {name:'Download', exact:true}).click();
    await page.locator('#notice[data-kind="error"]').waitFor();
    assert(await prompt.isVisible());
    assert.equal(handled, false, 'download failure must not dismiss onboarding');
    failDownload = false;
    failDismiss = true;
    await prompt.getByRole('button', {name:'Skip',exact:true}).click();
    await page.getByText('Isolated setup persistence failure', {exact:true}).waitFor();
    assert(await prompt.isVisible());
    failDismiss = false;
    await prompt.getByRole('button', {name:'Download',exact:true}).click();
    await prompt.waitFor({state:'detached'});
    assert.deepEqual(requested, ['arabic-q8.gguf','arabic-q8.gguf'], 'only the checked Arabic model should be requested');
    await page.reload();
    await page.locator('[data-setting-path="general.enabled"] input').waitFor();
    assert.equal(await prompt.count(), 0, 'handled prompt should not reappear on reopening');
    handled = false;
    await page.reload();
    await prompt.waitFor();
    const before = requested.length;
    await prompt.getByRole('button', {name:'Skip',exact:true}).click();
    await prompt.waitFor({state:'detached'});
    assert.equal(requested.length, before, 'Skip must not download');
    await page.reload();
    await page.locator('[data-setting-path="general.enabled"] input').waitFor();
    assert.equal(await prompt.count(), 0);
    handled = false;
    await page.reload();
    await prompt.waitFor();
    await prompt.getByRole('button', {name:'Download',exact:true}).click();
    await prompt.waitFor({state:'detached'});
    assert.deepEqual(requested.slice(-2), ['english-q4.gguf','arabic-q8.gguf'], 'both defaults should download only on explicit Download');
    assert.deepEqual(choices, [{english:false,arabic:true},{english:false,arabic:false},{english:true,arabic:true}], 'post the selected routing choices, including Skip');
    // A completed download refresh applies routing without discarding another settings draft.
    await page.locator('nav button[data-page="general"]').click();
    await page.locator('[data-setting-path="general.ui_language"] select').selectOption('ar');
    installed = true; pending = false;
    const completed = f.getConfig();
    completed.speech.selection_mode = 'follow_keyboard';
    completed.speech.language_models = {en:'english-q4.gguf',ar:'arabic-q8.gguf'};
    f.setConfig(completed);
    f.publishEvent('model_download_complete', {filename:'arabic-q8.gguf'});
    await page.locator('nav button[data-page="audio"]').click();
    await page.waitForFunction(() => document.querySelector('[data-setting-path="speech.selection_mode"] select')?.value === 'follow_keyboard');
    await page.locator('nav button[data-page="general"]').click();
    assert.equal(await page.locator('[data-setting-path="general.ui_language"] select').inputValue(), 'ar', 'model setup must preserve another unsaved draft');

    handled = false;
    f.options.online = false;
    await page.reload();
    await prompt.waitFor();
    assert(await prompt.getByRole('button', {name:'Download',exact:true}).isDisabled());
    assert(await prompt.getByRole('button', {name:'Skip',exact:true}).isEnabled());
    console.log('PASS first-run EN/AR checked, no automatic download, selected-only requests, failure retention, Skip/reopen persistence and localized layouts');
  } finally { await browser?.close(); await new Promise(resolve => f.server.close(resolve)); }
})().catch(error => { console.error(error); process.exitCode = 1; });
