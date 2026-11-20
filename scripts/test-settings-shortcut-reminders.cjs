const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require('playwright');
const f = require('./settings-fixture.cjs');
(async () => {
  await new Promise(resolve => f.server.listen(0, '127.0.0.1', resolve));
  const browser = await chromium.launch({headless:true});
  try {
    const page = await browser.newPage({viewport:{width:1280,height:900}});
    const errors=[];
    page.on('pageerror', e=>errors.push(e.message));
    const url=`http://127.0.0.1:${f.server.address().port}/#token=${f.token}`;
    await page.goto(url);
    const toggle=page.locator('[data-setting-path="general.shortcut_reminders"] input');
    await toggle.waitFor();
    assert(await toggle.isChecked());
    await toggle.uncheck();
    assert.equal(f.getConfig().general.shortcut_reminders,true, 'draft remains unsaved');
    await page.locator('#savebar button[type=submit]').click();
    await page.locator('#savebar').waitFor({state:'detached'});
    assert.equal(f.getConfig().general.shortcut_reminders,false);
    await page.reload();
    await toggle.waitFor();
    assert(!(await toggle.isChecked()));
    await page.getByRole('button',{name:'Show shortcuts',exact:true}).click();
    assert(f.actions.some(a=>a.action==='show_shortcuts'), 'manual show works with reminder disabled');
    const mode = page.getByRole('combobox', {name:'Show shortcuts mode',exact:true});
    assert.equal(await mode.inputValue(),'Hold to show');
    f.options.hotkey='CapsLock';
    await page.getByRole('button',{name:'Record Show shortcuts shortcut',exact:true}).click();
    await page.getByText(/Caps Lock alone uses Hold to show/).waitFor();
    assert(await mode.isDisabled());
    f.options.hotkey='CapsLock+J';
    await page.getByRole('button',{name:'Record Show shortcuts shortcut',exact:true}).click();
    await page.waitForFunction(()=>document.getElementById('general.show_shortcuts_hotkey').value==='CapsLock+J');
    await mode.click();
    await page.getByRole('option',{name:'Press to show',exact:true}).click();
    await page.locator('#savebar button[type=submit]').click();
    await page.locator('#savebar').waitFor({state:'detached'});
    assert.equal(f.getConfig().general.show_shortcuts_hotkey,'CapsLock+J');
    assert.equal(f.getConfig().general.show_shortcuts_mode,'press');
    assert.equal(f.getConfig().general.recording_mode,'hold');
    await page.reload();
    await mode.waitFor();
    assert.equal(await mode.inputValue(),'Press to show');
    const readerFields = page.locator('section[data-section="general"] [data-setting-path^="reader.shortcuts."]');
    assert.equal(await readerFields.count(),5);
    for (const [card, field] of [
      ['App-wide', 'general.show_shortcuts_hotkey'],
      ['Dictation', 'general.record_hotkey'],
      ['Reading', 'reader.shortcuts.read_selection'],
      ['Image text', 'reader.shortcuts.copy_region'],
    ]) {
      await page.getByRole('region', {name:card, exact:true})
        .locator(`[data-setting-path="${field}"]`).waitFor();
    }
    const copyShortcut=page.locator('#reader-shortcut-copy_region');
    await copyShortcut.waitFor();
    assert.equal(await copyShortcut.inputValue(),'None');
    f.options.hotkey=f.getConfig().general.record_hotkey;
    const recordCopy = page.locator('[data-setting-path="reader.shortcuts.copy_region"]').getByRole('button',{name:/^Record /});
    await recordCopy.click();
    await page.getByText('That shortcut is already assigned.',{exact:true}).waitFor();
    assert.equal(await copyShortcut.inputValue(),'None');
    f.options.hotkey='CapsLock+Shift+O';
    await recordCopy.click();
    await page.waitForFunction(()=>document.getElementById('reader-shortcut-copy_region').value==='CapsLock+Shift+O');
    await page.locator('#savebar button[type=submit]').click();
    await page.locator('#savebar').waitFor({state:'detached'});
    assert.equal(f.getReaderConfig().shortcuts.copy_region,'CapsLock+Shift+O');
    assert.equal(f.getReaderConfig().shortcuts.read_region,'Ctrl+Alt+O');
    await page.locator('nav button[data-page="reading"]').click();
    assert.equal(await page.locator('[data-setting-path^="reader.shortcuts."]').count(),0);
    await page.locator('#settings-search').fill('Copy screen text');
    await page.getByRole('option',{name:/Copy screen text/}).click();
    await readerFields.first().waitFor();
    assert.equal(await copyShortcut.inputValue(),'CapsLock+Shift+O');
    await page.getByRole('button',{name:'Reset Shortcuts to defaults',exact:true}).click();
    await page.waitForFunction(()=>document.getElementById('reader-shortcut-copy_region').value==='None');
    assert.equal(f.getReaderConfig().shortcuts.copy_region,'CapsLock+Shift+O','reset previews the draft');
    f.options.platform='linux';
    await page.reload();
    await toggle.waitFor();
    assert.equal(await mode.count(),0);
    assert.equal(await page.locator('[data-setting-path="linux.shortcut.shortcuts"]').count(),1);
    assert.equal(await page.locator('[data-setting-path="reader.shortcuts.copy_region"]').count(),1,'portal lists copy screen text even when unassigned');
    assert.equal(await page.locator('[data-setting-path="reader.shortcuts.copy_region"]').textContent().then(v=>v.includes('Copy screen text')),true);
    const dir=path.resolve('artifacts/shortcut-reminders/settings');
    fs.mkdirSync(dir,{recursive:true});
    for (const lang of ['en','ar']) {
      await page.locator('#ui-language').selectOption(lang);
      await page.waitForFunction(l=>document.documentElement.lang===l,lang);
      for (const width of [360,1280]) {
        await page.setViewportSize({width,height:900});
        await page.screenshot({path:path.join(dir,`${lang}-${width}.png`),fullPage:true});
        assert(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth+1));
      }
    }
    await page.locator('#settings-search').fill('تذكير بالاختصارات');
    await page.getByRole('option',{name:/تذكير بالاختصارات/}).waitFor();
    await page.locator('#settings-search').fill('');
    await page.locator('#ui-language').selectOption('en');
    f.options.platform = 'windows';
    f.getConfig().general.ui_theme = 'dark';
    await page.reload();
    await toggle.waitFor();
    await page.locator('nav button[data-page="general"] svg').waitFor();
    assert.equal(await page.locator('html').evaluate((root) => getComputedStyle(root).fontSize), '13px');
    assert.equal(await page.locator('html').evaluate((root) => getComputedStyle(root).colorScheme), 'dark');
    assert.equal(await page.locator('.rail').evaluate((rail) => getComputedStyle(rail).borderInlineEndWidth), '1px');
    assert.equal(await page.locator('.page-head').evaluate((header) => getComputedStyle(header).borderTopWidth), '0px');
    assert.equal(await page.locator('.shortcut-card h3').first().evaluate((heading) => getComputedStyle(heading).borderBlockEndWidth), '1px');
    assert.equal(await page.locator('.app-shell').evaluate((shell) => getComputedStyle(shell).backgroundImage), 'none');
    assert.equal(await page.locator('.group-head').first().evaluate((heading) => getComputedStyle(heading).backgroundImage), 'none');
    const dictation = page.locator('nav button[data-page="audio"]');
    await dictation.click();
    await page.getByRole('heading', { level:1, name:'Dictation' }).waitFor();
    assert.equal(await page.locator('.page-head p, .page-head pre').count(), 0);
    assert(await dictation.locator('small').isVisible());
    await page.locator('nav button[data-page="general"]').click();
    const sidebar = page.locator('.rail');
    const grip = page.getByRole('separator', {name:'Resize sidebar'});
    const start = await grip.boundingBox();
    const startWidth = (await sidebar.boundingBox()).width;
    const mainStartX = (await page.locator('.app-shell > main').boundingBox()).x;
    assert.equal(startWidth, 272);
    const pointerX = start.x + start.width / 2;
    const pointerY = start.y + start.height / 2;
    await page.mouse.move(pointerX, pointerY);
    await page.mouse.down();
    await page.mouse.move(pointerX + 64, pointerY, {steps:4});
    assert.equal((await sidebar.boundingBox()).width, startWidth + 64);
    assert.equal((await page.locator('.app-shell > main').boundingBox()).x, mainStartX + 64);
    await page.mouse.up();
    await page.mouse.move(pointerX + 100, pointerY);
    assert.equal((await sidebar.boundingBox()).width, startWidth + 64);
    await grip.press('ArrowLeft');
    assert.equal((await sidebar.boundingBox()).width, startWidth + 48);
    const captureTheme = async (theme, suffix = '') => {
      await page.waitForFunction((expected) => getComputedStyle(document.documentElement).colorScheme === expected, theme);
      await page.evaluate(async () => { await document.fonts.ready; window.scrollTo(0, 0); });
      const contrast = await page.evaluate(() => {
        const luminance = (color) => color.match(/\d+/g).map(Number).map((channel) => {
          const s = channel / 255;
          return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
        }).reduce((sum, value, index) => sum + value * [0.2126, 0.7152, 0.0722][index], 0);
        return [
          ['.page-head h1', '.page-head'], ['.group-head h2', '.group-head'],
          ['nav button.active small', 'nav button.active'], ['nav button:not(.active) small', 'nav button:not(.active)'],
          ['.shortcut-card h3', '.shortcut-card h3'], ['.shortcut-card label', '.shortcut-card'],
        ].map(([text, surface]) => {
          const foreground = luminance(getComputedStyle(document.querySelector(text)).color);
          const background = luminance(getComputedStyle(document.querySelector(surface)).backgroundColor);
          return [text, (Math.max(foreground, background) + 0.05) / (Math.min(foreground, background) + 0.05)];
        });
      });
      for (const [selector, ratio] of contrast) assert(ratio >= 4.5, `${theme} ${selector} contrast ${ratio}`);
      assert(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth));
      await page.screenshot({path:path.join(dir,`settings-${theme}${suffix}.png`), animations:'disabled'});
    };
    await grip.evaluate((element) => element.blur());
    await captureTheme('dark');
    await page.setViewportSize({width:390,height:850});
    assert(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth));
    assert.equal(await page.locator('.rail').evaluate((rail) => getComputedStyle(rail).borderBlockEndWidth), '1px');
    assert(!(await dictation.locator('small').isVisible()));
    assert(!(await grip.isVisible()));
    await captureTheme('dark', '-narrow');
    await page.locator('#ui-language').selectOption('ar');
    await page.waitForFunction(() => document.documentElement.dir === 'rtl');
    assert(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth));
    await page.setViewportSize({width:1280,height:900});
    const rtlGrip = page.locator('.sidebar-grip');
    await rtlGrip.press('ArrowLeft');
    assert.equal((await sidebar.boundingBox()).width, startWidth + 64);
    await page.locator('#ui-language').selectOption('en');
    await page.locator('[data-setting-path="general.ui_theme"]').getByRole('combobox').click();
    await page.locator('[data-combo-list="general.ui_theme"]').getByRole('option', {name:'Light', exact:true}).click();
    await page.locator('#savebar button[type=submit]').click();
    await page.locator('#savebar').waitFor({state:'detached'});
    assert.equal(f.getConfig().general.ui_theme, 'light');
    await page.reload();
    await toggle.waitFor();
    await captureTheme('light');
    await page.setViewportSize({width:390,height:850});
    await captureTheme('light', '-narrow');
    assert.deepEqual(errors,[]);
    console.log('PASS shortcut reminders: draft/save/reopen, independent show, shortcut recorder, KDE row, Arabic search, responsive screenshots');
  } finally {
    await browser.close(); f.closeEvents();
    await new Promise(resolve=>f.server.close(resolve));
  }
})().catch(e=>{console.error(e);process.exitCode=1;});
