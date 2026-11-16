const assert = require('node:assert/strict');
const { chromium } = require('playwright');
const f = require('./settings-fixture.cjs');
(async () => {
  let browser;
  try {
    await new Promise(resolve => f.server.listen(0, '127.0.0.1', resolve));
    browser = await chromium.launch({headless:true});
    const page = await browser.newPage();
    page.setDefaultTimeout(5000);
    const keys = {speech:'',ocr:'',stt:''}, writes = [], discoveries=[];
    await page.route('**/api/provider-discovery', async route=>{discoveries.push(route.request().postDataJSON());await route.fulfill({json:{models:[{id:'server-model'}],voices:[{id:'alloy'}]}});});
    await page.route('**/api/reader', async route => {
      const response = await route.fetch(), state = await response.json();
      Object.assign(state,{speech_key_saved:!!keys.speech,ocr_key_saved:!!keys.ocr,stt_key_saved:!!keys.stt});
      await route.fulfill({response,json:state});
    });
    await page.route('**/api/reader-secrets', async route => {
      const body = route.request().postDataJSON(); writes.push(body);
      for(const kind of Object.keys(keys)) {
        if(body[kind]) keys[kind]=body[kind];
        if(body[`clear_${kind}`]) keys[kind]='';
      }
      await route.fulfill({json:{ok:true}});
    });
    await page.goto(`http://127.0.0.1:${f.server.address().port}/#token=${f.token}`);
    await page.locator('nav button[data-page=models]').click();
    for(const [feature,provider,keyId,urlId,kind] of [
      ['tts','reading-provider','speech-key','speech-base_url','speech'],
      ['ocr','ocr-provider','ocr-key','ocr-base_url','ocr'],
      ['stt','speech.provider','stt-key','stt-base_url','stt'],
    ]) {
      await page.locator(`[data-catalog-feature=${feature}]`).click();
      const input=page.locator(`[data-setting-path="${provider}"] .combo-input`);
      await input.click();await input.fill('OpenAI');
      await page.getByRole('option',{name:'OpenAI-compatible',exact:true}).click();
      await page.locator(`#${urlId}`).fill(`http://100.85.156.101:8765/${feature}/v1`);
      await page.locator(`#${keyId}`).fill(`${feature}-fixture-key`);
      await page.waitForResponse(response=>response.url().endsWith("/api/provider-discovery")&&response.request().postDataJSON().api_key===`${feature}-fixture-key`);
      assert(discoveries.some(v=>v.feature===feature&&v.api_key===`${feature}-fixture-key`),'URL/key edits automatically discover with only that feature key');
      assert.equal(await page.getByRole('button',{name:'Fetch models and voices',exact:true}).count(),0);
      await page.getByRole('button',{name:'Refresh models',exact:true}).click();
      if(feature==='tts')await page.getByRole('button',{name:'Refresh voices',exact:true}).click();
      const model=page.locator(`[data-setting-path="${feature}-remote-model"] .combo-input`);await model.click();await model.fill('server-model');await model.press('Enter');
      const save=page.getByRole('button',{name:'Save connection',exact:true});
      if(feature==='tts'&&process.env.SIMPLE_STT_TEST_SCREENSHOT) await page.screenshot({path:process.env.SIMPLE_STT_TEST_SCREENSHOT,fullPage:true});
      await save.click();
      await page.waitForFunction(id=>document.getElementById(id)?.value==='',keyId);
      assert.equal(keys[kind],`${feature}-fixture-key`);
      assert(await page.getByRole('button',{name:'Clear key',exact:true}).isEnabled());
    }
    await page.reload();await page.locator('nav button[data-page=models]').click();
    await page.locator('[data-catalog-feature=tts]').click();
    assert.equal(await page.locator('#speech-base_url').inputValue(),'http://100.85.156.101:8765/tts/v1');
    await page.getByRole('button',{name:'Clear key',exact:true}).click();
    await page.waitForFunction(()=>document.querySelector('#speech-key')?.value==='');
    assert.equal(keys.speech,'');assert.equal(keys.ocr,'ocr-fixture-key');assert.equal(keys.stt,'stt-fixture-key');
    assert(await page.getByRole('button',{name:'Clear key',exact:true}).isDisabled());
    await page.locator('#speech-key').fill('unsaved-key');
    assert(await page.getByRole('button',{name:'Clear key',exact:true}).isEnabled());
    await page.getByRole('button',{name:'Clear key',exact:true}).click();
    await page.waitForFunction(()=>document.querySelector('#speech-key')?.value==='');
    assert.equal(writes.filter(v=>v.speech).length,1,'Clear must not save a pending replacement');
    for(const width of [360,768,1280]) {await page.setViewportSize({width,height:900});assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false,`provider controls fit ${width}px`);}
    console.log('PASS independent STT/TTS/OCR URL and key saves, reload, saved status and isolated clear');
  } finally {await browser?.close();await new Promise(resolve=>f.server.close(resolve));}
})().catch(error=>{console.error(error);process.exitCode=1;});
