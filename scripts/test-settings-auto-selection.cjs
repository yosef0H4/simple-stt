const assert = require('node:assert/strict');
const {chromium} = require('playwright');
const f = require('./settings-fixture.cjs');

(async () => {
  let browser;
  try {
    await new Promise(resolve => f.server.listen(0, '127.0.0.1', resolve));
    browser = await chromium.launch({headless:true});
    const page = await browser.newPage();
    const errors=[];page.on('pageerror',error=>errors.push(error.message));
    await page.goto(`http://127.0.0.1:${f.server.address().port}/#token=${f.token}`);
    await page.locator('[data-page=models]').click();
    await page.locator('[data-catalog-feature=tts]').click();
    const input=path=>page.locator(`[data-setting-path="${path}"] .combo-input`);
    await input('tts.add-language').waitFor();
    assert.equal(await page.locator('summary').filter({hasText:'Add language'}).count(),0);
    assert.equal(await input('tts.add-language').getAttribute('aria-label'),'Add language','empty state offers inline add');
    assert.equal(await page.locator('.engine-assignment[data-module=tts]').count(),0);
    f.installFixture('sherpa-cpu');f.installFixture('sherpa-cuda');f.installFixture('supertonic3');f.installFixture('kokoro');f.installFixture('parakeet-vulkan');f.installFixture('english-q8.gguf');
    const refresh=async()=>{await page.locator('[data-catalog-feature=tts]').click();await page.waitForResponse(response=>response.url().endsWith('/api/modules')&&response.ok());};
    await refresh();
    assert.equal(await page.locator('.engine-assignment[data-module=tts]').count(),0,'inventory refresh alone must not assign models');
    await input('tts.add-language').click();await input('tts.add-language').fill('English');
    await page.locator('[data-combo-list="tts.add-language"]').getByRole('option',{name:'English',exact:true}).click();
    await input('tts.en.runtime').click();
    await page.locator('[data-combo-list="tts.en.runtime"]').getByRole('option',{name:'Processor',exact:true}).click();
    await page.locator('[data-page=general]').click();
    await page.locator('#ui-language').selectOption('en');
    await page.locator('[data-page=models]').click();
    const saved=f.getReaderConfig();saved.revision++;
    saved.speech.selection.languages.ar={runtime:'sherpa-cuda',model:'supertonic3',device:'auto'};
    saved.speech.selection.languages.en={runtime:'sherpa-cuda',model:'kokoro',device:'auto'};
    f.getConfig().speech.language_runtimes.en='parakeet-vulkan';
    f.getConfig().speech.language_models.en='english-q8.gguf';
    f.options.selectionRevision=1;
    await refresh();
    await page.waitForFunction(()=>document.querySelector('[data-setting-path="tts.ar.model"] .combo-input')?.value==='Supertonic 3');
    assert.equal(await input('tts.ar.runtime').inputValue(),'NVIDIA');
    assert.equal(await input('tts.en.runtime').inputValue(),'Processor','automatic choices preserve a manual unsaved runtime');
    assert.equal(await input('tts.en.model').inputValue(),'Kokoro · English','a manual runtime still receives a compatible model in its empty slot');
    await page.locator('#savebar button').click();await page.locator('#savebar').waitFor({state:'detached'});
    assert.equal(f.getConfig().general.ui_language,'en','unrelated unsaved setting preserved');
    assert.equal(f.getConfig().speech.language_models.en,'english-q8.gguf');
    assert.equal(f.getReaderConfig().speech.selection.languages.en.runtime,'sherpa-cpu');
    assert.equal(f.getReaderConfig().speech.selection.languages.en.model,'kokoro');
    await page.reload();await page.locator('[data-page=models]').click();await page.locator('[data-catalog-feature=tts]').click();
    await input('tts.en.model').waitFor();assert.equal(await input('tts.en.model').inputValue(),'Kokoro · English','saved choices survive reopening');
    // An install completing during Save must merge once and retry the revision.
    let conflict=true;
    await page.route('**/api/reader-save',async route=>{
      if(!conflict)return route.continue();
      conflict=false;f.getReaderConfig().revision++;
      f.getReaderConfig().ocr.selection.single={runtime:'paddle-cuda',model:'paddle-ar-en',device:'auto'};
      await route.fulfill({status:409,json:{error:'configuration revision conflict: concurrent model installation'}});
    });
    await input('tts.en.model').click();await page.locator('[data-combo-list="tts.en.model"]').getByRole('option',{name:'None',exact:true}).click();
    await page.locator('#savebar button').click();await page.locator('#savebar').waitFor({state:'detached'});
    assert.equal(conflict,false);assert.equal(f.getReaderConfig().speech.selection.languages.en.model,null,'manual clear survives concurrent auto-selection');
    assert.equal(f.getReaderConfig().ocr.selection.single.model,'paddle-ar-en');
    assert.deepEqual(errors,[]);
    console.log('PASS automatic choices appear immediately, preserve manual drafts, save/reopen and merge a concurrent install');
  } finally {await browser?.close();f.closeEvents();await new Promise(resolve=>f.server.close(resolve));}
})().catch(error=>{console.error(error.stack||error);process.exitCode=1;});
