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
    f.options.hotkey='CapsLock+J';
    await page.getByRole('button',{name:'Record Show shortcuts shortcut',exact:true}).click();
    await page.waitForFunction(()=>document.getElementById('general.show_shortcuts_hotkey').value==='CapsLock+J');
    await page.locator('#savebar button[type=submit]').click();
    await page.locator('#savebar').waitFor({state:'detached'});
    assert.equal(f.getConfig().general.show_shortcuts_hotkey,'CapsLock+J');
    const readerFields = page.locator('section[data-section="general"] [data-setting-path^="reader.shortcuts."]');
    assert.equal(await readerFields.count(),5);
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
    assert.deepEqual(errors,[]);
    console.log('PASS shortcut reminders: draft/save/reopen, independent show, shortcut recorder, KDE row, Arabic search, responsive screenshots');
  } finally {
    await browser.close(); f.closeEvents();
    await new Promise(resolve=>f.server.close(resolve));
  }
})().catch(e=>{console.error(e);process.exitCode=1;});
