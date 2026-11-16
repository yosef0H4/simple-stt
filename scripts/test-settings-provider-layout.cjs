const assert=require('node:assert/strict');
const {chromium}=require('playwright');
const f=require('./settings-fixture.cjs');
require('node:fs').mkdirSync('artifacts/model-pages',{recursive:true});
(async()=>{
 await new Promise(r=>f.server.listen(0,'127.0.0.1',r));const browser=await chromium.launch();
 try{const page=await browser.newPage();await page.goto(`http://127.0.0.1:${f.server.address().port}/#token=${f.token}`);
 const go=async(p)=>{await page.locator(`[data-page=${p}]`).click();};
 await go('models');
 for(const kind of ['model','runtime']){await page.locator(`[data-download-kind=${kind}]`).click();assert.equal(await page.locator('.catalog').count(),1);await page.locator(`#${kind}-search`).waitFor();await page.screenshot({path:`artifacts/model-pages/${kind}.png`,fullPage:true});}
 assert.equal(await page.locator('.selection-section').count(),0,'runtime page has no model setup');
 await go('general');await go('models');await page.locator('#runtime-search').waitFor();
 await page.reload();await go('models');await page.locator('#runtime-search').waitFor();
 await page.locator('[data-download-kind=model]').click();assert.equal(await page.locator('.selection-section').count(),1);
 await page.locator('[data-catalog-feature=tts]').click();const input=page.locator('[data-setting-path=reading-provider] .combo-input');await input.click();await input.fill('OpenAI');await page.getByRole('option',{name:'OpenAI-compatible',exact:true}).click();
 await page.locator('#speech-base_url').fill('http://localhost:8080/v1');await page.getByRole('button',{name:'Fetch models and voices',exact:true}).click();
 const choose=async(path,query,label)=>{const i=page.locator(`[data-setting-path="${path}"] .combo-input`);await i.click();await i.fill(query);await page.locator(`[data-combo-list="${path}"]`).getByRole('option',{name:label,exact:true}).click();};
 await choose('tts-remote-model','Server','Server model');await choose('tts-remote-voice','Bella','Bella');
 const custom=page.locator('[data-setting-path=tts-remote-model] .combo-input');await custom.fill('custom-model');await page.keyboard.press('Tab');assert.equal(await custom.inputValue(),'custom-model');
 const order=await page.locator('#speech-base_url').evaluate(e=>e.closest('.field').nextElementSibling.querySelector('input').id);assert.equal(order,'speech-key','URL and key must be adjacent');
 await go('advanced');assert(await page.locator('.reading-actions button').count()>=2,'server action buttons rendered');
 for(const width of [360,768,1280]){await page.setViewportSize({width,height:900});const issues=await page.locator('.reading-actions').evaluateAll(groups=>groups.flatMap(g=>{const buttons=[...g.querySelectorAll('button')].map(b=>b.getBoundingClientRect());return buttons.slice(1).filter((b,i)=>{const a=buttons[i];return Math.abs(a.top-b.top)<2&&Math.max(a.left,b.left)-Math.min(a.right,b.right)<7;}).map(()=> 'touching buttons');}));assert.deepEqual(issues,[],`button spacing at ${width}`);assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);}
 console.log('PASS separate download views, discovered/custom provider IDs, adjacent credentials and responsive button gaps');
 }finally{await browser.close();f.server.close();}
})().catch(e=>{console.error(e);process.exit(1)});
