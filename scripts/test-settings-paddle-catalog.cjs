const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require('playwright');
const f = require('./settings-fixture.cjs');
const canonical = require('../assets/catalog/modules.json');
const downloads = require('../assets/catalog/downloads.json');

(async () => {
  let browser;
  try {
    const ocr = canonical.models.filter(model => model.feature === 'ocr');
    assert.equal(ocr.length, 34, 'canonical OCR catalog');
    for (const asset of downloads) {
      if (!f.assets.some(item => item.id === asset.id)) {
        f.assets.push({ ...asset, installed: false, available: true, directory: `/assets/${asset.id}` });
      }
    }
    f.options.recommendations = { 'paddle-ar-en': 'available_ocr' };
    await new Promise(resolve => f.server.listen(0, '127.0.0.1', resolve));
    browser = await chromium.launch({ headless: true });
    const page = await browser.newPage({ viewport: { width: 360, height: 900 } });
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.route('**/api/modules', route => {
      const inventory = f.inventory();
      inventory.models = inventory.models.filter(model => model.feature !== 'ocr');
      inventory.models.push(...ocr.map(model => ({
        ...model,
        installed: !!f.assets.find(asset => asset.id === model.asset)?.installed,
        recommended: model.id === 'paddle-ar-en',
        recommendation: model.id === 'paddle-ar-en' ? 'available_ocr' : null,
      })));
      return route.fulfill({ json: inventory });
    });
    await page.goto(`http://127.0.0.1:${f.server.address().port}/#token=${f.token}`);
    await page.getByRole('heading', { name: 'General' }).waitFor();
    await page.locator('[data-page=models]').click();
    const choose = async feature => {
      await page.locator(`[data-catalog-feature=${feature}]`).click();
      await page.locator(`[data-catalog="${feature}.model"]`).waitFor();
    };
    const refresh = async () => {
      const inventory = page.waitForResponse('**/api/modules');
      await page.locator('.catalog .catalog-toolbar button').click();
      await inventory;
    };
    const row = id => page.locator(`[data-model-id="${id}"]`);
    const section = page.locator('[data-catalog="ocr.model"]');
    const search = page.locator('#model-search');
    const showAll = async () => {
      const more = section.getByRole('button', { name: 'Show more', exact: true });
      while (await more.isVisible()) await more.click();
    };
    const idFor = filename => {
      const entry = ocr.find(model => model.paddle.recognizer === filename);
      assert(entry, `missing recognizer ${filename}`);
      return entry.id;
    };
    await choose('ocr');
    await page.getByText('Install a runtime first.', { exact: true }).waitFor();
    await row('paddle-ar-en').waitFor();
    await page.locator('[data-download-kind=runtime]').click();
    for (const feature of ['tts', 'ocr']) {
      await page.locator(`[data-catalog-feature=${feature}]`).click();
      const runtimeSection = page.locator(`[data-catalog="${feature}.runtime"]`);
      for (const [id, label] of [[feature === 'tts' ? 'sherpa-cpu' : 'paddle-cpu', 'Sherpa ONNX · CPU'], [feature === 'tts' ? 'sherpa-cuda' : 'paddle-cuda', 'Sherpa ONNX · CUDA']]) {
        assert.equal((await runtimeSection.locator(`[data-model-id="${id}"] small`).textContent()).trim(), label);
      }
    }
    await page.locator('[data-download-kind=model]').click();
    f.installFixture('sherpa-cpu');
    await refresh();
    await choose('ocr');
    await row('paddle-ar-en').waitFor();
    assert.equal(await section.locator('.model-result').count(), 30, 'first page is bounded');
    assert.equal(await section.locator('.model-result').first().getAttribute('data-model-id'), 'paddle-ar-en');
    await showAll();
    assert.equal(await section.locator('.model-result').count(), 34);
    assert.deepEqual(new Set(await section.locator('.model-result').evaluateAll(nodes => nodes.map(node => node.dataset.modelId))), new Set(ocr.map(model => model.id)));
    assert.deepEqual(await section.locator('.model-result:has(.recommendation)').evaluateAll(nodes => nodes.map(node => node.dataset.modelId)), ['paddle-ar-en']);
    for (const [query, expected] of [
      ['paddle', 'paddle-ar-en'],
      ['latin', idFor('latin_pp-ocrv5_mobile_rec.onnx')],
      ['French', idFor('latin_pp-ocrv5_mobile_rec.onnx')],
      ['Greek', idFor('el_pp-ocrv5_mobile_rec.onnx')],
      ['Kannada', idFor('ka_pp-ocrv3_mobile_rec.onnx')],
      ['v6', idFor('pp-ocrv6_small_rec.onnx')],
    ]) {
      await search.fill(query);
      await showAll();
      assert.equal(await row(expected).count(), 1, `search ${query}`);
    }
    const latinId = idFor('latin_pp-ocrv5_mobile_rec.onnx');
    const v6Id = idFor('pp-ocrv6_small_rec.onnx');
    f.installFixture('paddle-ar-en');
    f.installFixture(latinId);
    await refresh();
    await search.fill('');
    await section.locator('.installed-filter input').check();
    assert.deepEqual(new Set(await section.locator('.model-result').evaluateAll(nodes => nodes.map(node => node.dataset.modelId))), new Set(['paddle-ar-en', latinId]));
    await section.locator('.installed-filter input').uncheck();
    const modelInput = page.locator('[data-setting-path="ocr.single.model"] .combo-input');
    await modelInput.click();
    await modelInput.fill('Latin');
    const choices = page.locator('[data-combo-list="ocr.single.model"]');
    await choices.getByRole('option', { name: /Latin/ }).waitFor();
    assert.equal(await choices.getByRole('option', { name: /Latin/ }).count(), 1, 'only installed Latin recognizer is selectable');
    await modelInput.fill('v6');
    assert.equal(await choices.getByRole('option', { name: /v6/i }).count(), 0, 'uninstalled model is not selectable');
    await modelInput.fill('Latin');
    const speechBefore = { fallback_to_local: true, ...structuredClone(f.getReaderConfig().speech) };
    const sttBefore = structuredClone(f.getConfig().speech);
    await choices.getByRole('option', { name: /Latin/ }).click();
    await page.locator('#savebar button').click();
    await page.locator('#savebar').waitFor({ state: 'detached' });
    assert.equal(f.getReaderConfig().ocr.selection.single.model, latinId);
    assert.deepEqual(f.getReaderConfig().speech, speechBefore, 'OCR selection preserves reader selection');
    assert.deepEqual(f.getConfig().speech, sttBefore, 'OCR selection preserves dictation');
    await search.fill('v6');
    await row(v6Id).waitFor();
    const summary = row(v6Id).locator('.model-copy > span').filter({ has: page.locator('.help') });
    assert.match(await summary.textContent(), /\+\d+/, 'long language list is summarized');
    const help = summary.locator('.help button');
    assert.match(await help.getAttribute('title'), /French/, 'full language list remains accessible');
    if (process.env.SETTINGS_VISUAL_REVIEW === '1') {
      const output = path.resolve('artifacts/paddle-catalog/settings');
      fs.mkdirSync(output, { recursive: true });
      for (const locale of ['en', 'ar']) {
        await page.locator('[data-page=general]').click();
        await page.locator('#ui-language').selectOption(locale);
        for (const theme of ['light', 'dark']) {
        await page.locator('[data-setting-path="general.ui_theme"] [role=combobox]').click();
        await page.getByRole('option', { name: new RegExp(theme === 'light' ? 'Light|فاتح' : 'Dark|داكن', 'i') }).click();
          for (const width of [360, 768, 1280]) {
            await page.setViewportSize({ width, height: 900 });
            await page.locator('[data-page=models]').click();
            await choose('ocr');
            await search.fill('v6');
            await row(v6Id).waitFor();
            await page.evaluate(() => document.fonts.ready);
            assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), `${locale}/${theme}/${width} overflow`);
            await page.screenshot({ path: path.join(output, `${locale}-${theme}-${width}.png`), fullPage: true });
            await page.locator('[data-page=general]').click();
          }
        }
      }
    }
    assert.deepEqual(errors, []);
    console.log('PASS Paddle OCR catalog gating, pagination, search, shared runtimes, selection and language summary');
  } finally {
    await browser?.close();
    f.closeEvents();
    await new Promise(resolve => f.server.close(resolve));
  }
})().catch(error => { console.error(error.stack || error); process.exitCode = 1; });
