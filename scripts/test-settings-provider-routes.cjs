const assert=require('node:assert/strict');
const {chromium}=require('playwright');
const f=require('./settings-fixture.cjs');
(async()=>{
 let browser;
 try{
  f.installFixture('sherpa-cpu');f.installFixture('supertonic3');
  await new Promise(resolve=>f.server.listen(0,'127.0.0.1',resolve));
  browser=await chromium.launch({headless:true});
  const page=await browser.newPage({viewport:{width:1280,height:900}});
  const errors=[];page.on('pageerror',error=>errors.push(error.message));
  const credentials=new Map();
  await page.route('**/api/connection-secret',async route=>{
   const payload=route.request().postDataJSON();
   if(payload.clear)credentials.delete(payload.slot);
   if(payload.value)credentials.set(payload.slot,payload.value);
   await route.fulfill({json:{saved:credentials.has(payload.slot)}});
  });
  const choose=async(path,query,name)=>{
   const input=page.locator(`[data-setting-path="${path}"] .combo-input`);
   await input.fill(query);
   await page.locator(`[data-combo-list="${path}"]`).getByRole('option',{name}).click();
  };
  const exact=async(path,value)=>{const input=page.locator(`[data-setting-path="${path}"] .combo-input`);await input.fill(value);await input.press('Enter');};
  const save=async()=>{await page.locator('#savebar button[type=submit]').click();await page.waitForFunction(()=>!document.querySelector('#savebar'));};
  const tts=async()=>{await page.locator('[data-page=models]').click();await page.locator('[data-catalog-feature=tts]').click();};
  await page.goto(`http://127.0.0.1:${f.server.address().port}/#token=${f.token}`);
  await tts();
  await choose('reading-provider','OpenAI',/^OpenAI-compatible$/);
  await page.locator('#speech-base_url').fill('https://default.example/v1');
  await exact('tts-remote-model','default-model');
  await exact('tts-remote-voice','default-voice');
  await choose('reading-provider','Local',/Local/);
  await save();await page.reload();await tts();
  await choose('reading-provider','OpenAI',/^OpenAI-compatible$/);
  assert.equal(await page.locator('#speech-base_url').inputValue(),'https://default.example/v1');
  assert.equal(await page.locator('[data-setting-path=tts-remote-model] .combo-input').inputValue(),'default-model');
  await choose('reading-provider','Local',/Local/);
  for(const language of ['ar','en']){
   if(language==='en')await page.getByRole('button',{name:'Add language',exact:true}).click();
   await choose('tts.add-language',language==='ar'?'Arabic':'English',language==='ar'?/^Arabic$/:/^English$/);
   await page.locator(`[id="tts.${language}-route"]`).selectOption('remote');
   await page.locator(`[id="tts.${language}.primary-url"]`).fill(`https://${language}.example/v1`);
   await exact(`tts.${language}.primary-model`,`${language}-tts`);
   await exact(`tts.${language}.primary-voice`,`${language}-voice`);
   await page.locator(`[id="tts.${language}.primary-key"]`).fill(`${language}-private-token`);
   await page.locator(`[id="tts.${language}.primary-key"]`).locator('..').getByRole('button',{name:'Save connection',exact:true}).click();
   await page.waitForFunction(()=>!document.querySelector('#savebar'));
  }
  assert.equal(credentials.get('tts.ar.primary'),'ar-private-token');
  assert.equal(credentials.get('tts.en.primary'),'en-private-token');
  const arabic=page.locator('[data-route-card="tts.ar"]');
  const localFallback=arabic.locator('[data-route-card="tts.ar.local-fallback"]');
  await arabic.getByRole('heading',{name:'Arabic',exact:true}).waitFor();
  await page.locator('[data-route-card="tts.en"]').getByRole('heading',{name:'English',exact:true}).waitFor();
  assert.equal(await page.locator('[data-catalog-selection=tts] .field').first().locator('select').getAttribute('id'),'tts-mode');
  assert.equal(await localFallback.count(),0);
  await page.locator('[id="tts.ar-local-fallback"]').check();
  await localFallback.getByRole('heading',{name:'Local fallback models'}).waitFor();
  await choose('tts.ar.runtime','CPU',/^Processor$/);
  await choose('tts.ar.model','Supertonic',/^Supertonic 3$/);
  assert.equal(await arabic.getByText('Use a fallback server',{exact:true}).count(),0);
  await save();await page.reload();await tts();
  await localFallback.waitFor();
  assert.equal(await localFallback.locator('[data-setting-path="tts.ar.model"] .combo-input').inputValue(),'Supertonic 3');
  assert.equal(await page.locator('[id="tts.ar.primary-url"]').inputValue(),'https://ar.example/v1');
  assert.equal(await arabic.locator('input[id$="-url"]').count(),1);
  await page.locator('[id="tts.ar-route"]').selectOption('local');await save();await page.reload();await tts();
  await page.locator('[id="tts.ar-route"]').selectOption('remote');
  assert.equal(await page.locator('[id="tts.ar.primary-url"]').inputValue(),'https://ar.example/v1');
  await localFallback.waitFor();
  await save();
  await page.locator('[data-catalog-feature=stt]').click();
  await page.locator('[id="stt.ar-route"]').selectOption('remote');
  await page.locator('[id="stt.ar.primary-url"]').fill('https://dictation-ar.example/v1');
  await exact('stt.ar.primary-model','arabic-stt');await save();
  assert.equal(await page.getByText('Use a fallback server',{exact:true}).count(),0);
  assert.equal(f.getConfig().speech.remote_languages.ar.primary.model,'arabic-stt');
  assert.equal(f.getReaderConfig().speech.selection.languages.en.remote.primary.voice,'en-voice');
  assert(!JSON.stringify(f.getReaderConfig()).includes('private-token'));
  for(const width of [360,768,1280]){await page.setViewportSize({width,height:900});assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);}
  assert.deepEqual(errors,[]);
  console.log('PASS saved default and per-language local/remote routes, independent credentials, model/voice selection, fallback, reload and layout');
 }finally{if(browser)await browser.close();f.closeEvents();await new Promise(resolve=>f.server.close(resolve));}
})().catch(error=>{console.error(error);process.exitCode=1;});
