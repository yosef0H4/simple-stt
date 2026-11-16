const assert=require('node:assert/strict');
const {chromium}=require('playwright');
const f=require('./settings-fixture.cjs');
(async()=>{
 const {ActionRecorder}=await import('../web/settings/src/lib/action-recorder.ts');
 const recorder=new ActionRecorder();
 for(const [key,down,at] of [['Space',true,5000],['Space',false,5050],['Space',true,5250],['Space',false,5300]])recorder.event(key,down,at);
 assert.deepEqual(recorder.finish().events.map(e=>e.at_ms),[0,50,250,300]);
 assert.equal(recorder.finish().events.at(-1).at_ms,300,'finishing later must not add idle time');
 const held=new ActionRecorder();held.event('ShiftLeft',true,0);assert.equal(held.finish(),null);held.event('ShiftLeft',false,80);assert(held.finish());
 let browser;
 try{
  await new Promise(resolve=>f.server.listen(0,'127.0.0.1',resolve));browser=await chromium.launch({headless:true});const page=await browser.newPage();page.setDefaultTimeout(6000);
  await page.goto(`http://127.0.0.1:${f.server.address().port}/#token=${f.token}`);
  await page.locator('nav button[data-page=reading]').click();
  assert.equal(await page.locator('#recorded-action').inputValue(),'Space');
  await page.getByRole('button',{name:'Record',exact:true}).click();
  await page.keyboard.press('Space');await page.waitForTimeout(140);await page.keyboard.press('Space');await page.waitForTimeout(200);await page.keyboard.press('Escape');
  assert.equal(await page.locator('#recorded-action').inputValue(),'Space → Space');
  await page.getByRole('button',{name:'Record',exact:true}).click();await page.keyboard.press('KeyA');await page.locator('#read-along-action').focus();
  assert.equal(await page.locator('#recorded-action').inputValue(),'Space → Space','blur discards the incomplete replacement');
  await page.getByRole('button',{name:'Record',exact:true}).click();await page.keyboard.press('Escape');
  assert.equal(await page.locator('#recorded-action').inputValue(),'Space → Space','empty recording keeps the previous action');
  await page.locator('#savebar button[type=submit]').click();
  await page.waitForFunction(()=>!document.querySelector('#savebar'));
  await page.reload();await page.locator('nav button[data-page=reading]').click();
  assert.equal(await page.locator('#recorded-action').inputValue(),'Space → Space');
  for(const width of [360,768,1280]){await page.setViewportSize({width,height:900});assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);}
  if(process.env.SIMPLE_STT_TEST_SCREENSHOT)await page.screenshot({path:process.env.SIMPLE_STT_TEST_SCREENSHOT,fullPage:true});
  await page.getByRole('button',{name:'Reset',exact:true}).click();assert.equal(await page.locator('#recorded-action').inputValue(),'Space');
  console.log('PASS timing trim, release pairing, focused recording, Escape stop, blur/empty cancellation, save/reload and responsive controls');
 }finally{await browser?.close();await new Promise(resolve=>f.server.close(resolve));}
})().catch(error=>{console.error(error);process.exitCode=1});
