const assert=require('node:assert/strict');
const {chromium}=require('playwright');
const f=require('./settings-fixture.cjs');
(async()=>{
 let browser;
 try{
  await new Promise(resolve=>f.server.listen(0,'127.0.0.1',resolve));
  browser=await chromium.launch({headless:true});const page=await browser.newPage();
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.goto(`http://127.0.0.1:${f.server.address().port}/#token=${f.token}`);
  const reading=async()=>{await page.locator('[data-page=reading]').click();};
  await reading();
  const words='Name\nمرحبا\nA literal phrase\na+b';
  await page.locator('#tts-excluded-words').fill(words);
  await page.locator('#savebar button[type=submit]').click();await page.locator('#savebar').waitFor({state:'detached'});
  assert.deepEqual(f.getReaderConfig().speech.excluded_words,words.split('\n'));
  await page.reload();await reading();assert.equal(await page.locator('#tts-excluded-words').inputValue(),words);
  for(const width of [360,768,1280]){await page.setViewportSize({width,height:900});assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);}
  await page.locator('#tts-excluded-words').fill('');
  await page.locator('#savebar button[type=submit]').click();await page.locator('#savebar').waitFor({state:'detached'});
  await page.reload();await reading();assert.equal(await page.locator('#tts-excluded-words').inputValue(),'');
  assert.deepEqual(errors,[]);console.log('PASS TTS exclusion list editing, literal entries, save/reload, clear and responsive layout');
 }finally{await browser?.close();f.closeEvents();await new Promise(resolve=>f.server.close(resolve));}
})().catch(error=>{console.error(error);process.exitCode=1;});
