const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const {chromium} = require('playwright');
const f = require('./settings-fixture.cjs');

(async () => {
  let browser;
  try {
    f.options.recommendations = {'parakeet-cuda':'nvidia_driver_detected','sherpa-cuda':'nvidia_driver_detected','sherpa-cpu':null,'paddle-cpu':'small_capture_cpu','kokoro':null,'supertonic3':'cpu_voice','paddle-ar-en':'available_ocr'};
    await new Promise(resolve => f.server.listen(0, '127.0.0.1', resolve));
    browser = await chromium.launch({headless:true});
    const page = await browser.newPage({viewport:{width:360,height:900}});
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    let whisperInstalled = false;
    await page.route('**/api/modules', route => {
      const catalog = f.inventory();
      catalog.runtimes.push({id:'whisper-gpu',feature:'stt',engine:'whisper',backend:'vulkan',label:'Whisper Vulkan',asset:null,installed:whisperInstalled,directory:null});
      catalog.models.push({id:'whisper-tiny',feature:'stt',engine:'whisper',label:'Whisper Tiny',languages:['en'],asset:null,installed:false});
      return route.fulfill({json:catalog});
    });
    await page.goto(`http://127.0.0.1:${f.server.address().port}/#token=${f.token}`);
    await page.getByRole('heading',{name:'General'}).waitFor();
    assert.equal(await page.locator('[role=dialog]').count(),0,'empty installation does not open setup');
    await page.locator('[data-page=models]').click();
    let search, section;
    const choose = async (feature, kind) => {
      await page.locator(`[data-catalog-feature=${feature}]`).click();
      section = page.locator(`[data-catalog="${feature}.${kind}"]`);
      search = page.locator(`#${kind}-search`);
      await section.waitFor();
    };
    const refresh = async () => {await page.locator('#runtime-section .catalog-toolbar button').click();await page.waitForFunction(()=>document.querySelector('#runtime-section .catalog-toolbar button')?.getAttribute('aria-busy')==='false');};
    const row = id => page.locator(`[data-model-id="${id}"]`);
    for (const feature of ['stt','tts','ocr']) {
      await choose(feature,'model');
      const runtime = page.locator(`[data-catalog="${feature}.runtime"]`);
      const model = page.locator(`[data-catalog="${feature}.model"]`);
      assert(await runtime.isVisible() && await model.isVisible(), 'both sections are visible');
      const selection = page.locator(`[data-catalog-selection="${feature}"]`);
      await selection.locator(`#${feature}-mode`).waitFor();
      assert.equal(await selection.locator('.engine-assignment').count(),feature==='tts'?0:1,`${feature} has at most one active editor`);
      assert(await selection.evaluate((el, runtimeEl) => !!(el.compareDocumentPosition(runtimeEl) & Node.DOCUMENT_POSITION_FOLLOWING), await runtime.elementHandle()), 'selection precedes runtime installation');
      assert.equal(await selection.locator('.module-download-link').count(), 0, 'no link back to this page');
      assert(await runtime.evaluate((el, modelEl) => !!(el.compareDocumentPosition(modelEl) & Node.DOCUMENT_POSITION_FOLLOWING), await model.elementHandle()), 'runtimes precede models');
      assert.equal(await runtime.locator('h2').textContent(),'Runtimes');
      assert.equal(await model.locator('h2').textContent(),'Models');
      assert.equal(await page.locator('[data-catalog-kind]').count(),0,'no runtime/model tabs');
      await model.getByText('Install a runtime first.',{exact:true}).waitFor();
      assert.equal(await model.locator('.model-result').count(),0,'new models require a compatible runtime');
      assert.equal(await runtime.locator('.model-result').count(),feature==='stt'?3:2);
    }
    await choose('stt','runtime');
    await page.locator('#stt-mode').selectOption('single_model');
    await choose('tts','runtime');
    await page.locator('#tts-mode').selectOption('single_model');
    await choose('ocr','runtime');
    await choose('tts','runtime');
    assert.equal(await page.locator('#tts-mode').inputValue(),'single_model','reader draft survives feature switch');
    await choose('stt','runtime');
    assert.equal(await page.locator('#stt-mode').inputValue(),'single_model','dictation draft survives feature switch');
    await page.locator('#savebar button').click();
    await page.locator('#savebar').waitFor({state:'detached'});
    assert.equal(f.getConfig().speech.selection_mode,'single_model');
    assert.equal(f.getReaderConfig().speech.selection.mode,'single_model');
    await page.locator('#stt-mode').selectOption('follow_keyboard');
    await page.locator('#savebar button').click();
    await page.locator('#savebar').waitFor({state:'detached'});
    assert.equal(await row('parakeet-cuda').locator('strong').textContent(),'NVIDIA');
    assert.match(await row('parakeet-cuda').locator('small').textContent(),/CUDA/,'technical name is secondary');
    await search.fill('recommended');
    assert.deepEqual(await section.locator('.model-result').evaluateAll(nodes=>nodes.map(node=>node.dataset.modelId)),['parakeet-cuda']);
    await search.fill('NVIDIA'); assert.equal(await row('parakeet-cuda').count(),1);
    assert.equal(await page.locator('#model-search').inputValue(),'','runtime and model searches are independent');
    await search.fill('');
    await row('parakeet-cuda').getByRole('button',{name:'Download',exact:true}).click();
    await row('parakeet-vulkan').getByRole('button',{name:'Download',exact:true}).click();
    await row('parakeet-vulkan').locator('progress').waitFor();
    assert.equal(await page.locator('.model-result progress').count(),2,'independent progress belongs to each item');
    assert.equal(await page.locator('.download-summary').count(),0,'no detached global banner');
    assert.equal(await row('parakeet-cuda').locator('progress').getAttribute('value'),String(Math.floor(f.assets.find(a=>a.id==='parakeet-cuda').bytes/4)));
    if(process.env.SETTINGS_VISUAL_REVIEW==='1') {
      const output=path.resolve('artifacts/catalog-ui');fs.mkdirSync(output,{recursive:true});
      for(const locale of ['en','ar']) for(const theme of ['light','dark']) {
        await page.locator('[data-page=general]').click();await page.locator('#ui-language').selectOption(locale);await page.locator('[data-setting-path="general.ui_theme"] select').selectOption(theme);await page.locator('[data-page=models]').click();await choose('stt','runtime');
        for(const width of [360,1280]) {await page.setViewportSize({width,height:900});await page.evaluate(()=>document.fonts.ready);assert.equal(await page.locator('.model-result progress').count(),2);assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1));assert(await page.locator('.model-result progress').evaluateAll(nodes=>nodes.every(node=>{const bar=node.getBoundingClientRect(),row=node.closest('article').getBoundingClientRect();return bar.y>=row.y&&bar.bottom<=row.bottom&&bar.x>=row.x&&bar.right<=row.right+1;})),'progress must stay inside its downloading item');await page.screenshot({path:path.join(output,`${locale}-${theme}-${width}-downloading.png`),fullPage:true});}
      }
      await page.locator('[data-page=general]').click();await page.locator('#ui-language').selectOption('en');await page.locator('[data-setting-path="general.ui_theme"] select').selectOption('auto');await page.setViewportSize({width:360,height:900});await page.locator('[data-page=models]').click();
    }
    await choose('tts','runtime');
    await row('sherpa-cpu').getByRole('button',{name:'Download',exact:true}).click();
    await row('sherpa-cpu').getByText('Queued',{exact:true}).waitFor();
    await choose('stt','runtime');
    await row('parakeet-cuda').getByRole('button',{name:'Cancel NVIDIA',exact:true}).click();
    await row('parakeet-cuda').locator('progress').waitFor({state:'detached'});
    assert(f.actions.some(action=>action.action==='cancel'&&action.id==='parakeet-cuda'));
    assert.equal(await row('parakeet-vulkan').locator('progress').count(),1,'cancel one does not cancel another');
    await choose('tts','runtime');
    await row('sherpa-cpu').getByText('Downloading…',{exact:true}).waitFor();
    f.installFixture('sherpa-cpu'); await refresh();
    await choose('tts','model');
    await row('supertonic3').waitFor();
    await search.fill('recommended'); assert.equal(await row('supertonic3').count(),1); assert.equal(await row('kokoro').count(),0,'CPU recommendation comes from server policy');
    await search.fill('kokoro');
    await row('kokoro').getByRole('button',{name:'Download',exact:true}).click();
    await choose('ocr','model');
    await search.fill('paddle'); assert.equal(await row('paddle-ar-en').count(),1,'shared runtime enables compatible OCR models');
    await choose('tts','model'); assert.equal(await search.inputValue(),'kokoro','each catalog retains its own search');
    f.installFixture('kokoro'); await refresh();
    await choose('tts','runtime');
    await row('sherpa-cpu').getByRole('button',{name:'Remove',exact:true}).click();
    await choose('tts','model');
    await row('kokoro').getByText('Runtime missing',{exact:true}).waitFor();
    if(process.env.SETTINGS_VISUAL_REVIEW==='1')await page.screenshot({path:path.resolve('artifacts/catalog-ui/en-orphan-model.png'),fullPage:true});
    assert.equal(await row('supertonic3').count(),0,'undownloaded models stay hidden without runtime');
    await row('kokoro').getByRole('button',{name:'Remove',exact:true}).click();
    await row('kokoro').waitFor({state:'detached'});
    assert(f.actions.some(action=>action.action==='remove'&&action.id==='kokoro'),'orphan model remains removable');
    await section.getByText('Install a runtime first.',{exact:true}).waitFor();
    assert.equal(await section.locator('.model-result').count(),0);
    f.installFixture('parakeet-vulkan'); await choose('stt','runtime'); await refresh();
    await choose('stt','model');
    assert.equal(await row('english-q8.gguf').count(),1); assert.equal(await row('whisper-tiny').count(),0,'wrong engine is hidden');
    f.options.modelDownloadDelay=10000;
    await row('english-q8.gguf').getByRole('button',{name:'Download',exact:true}).click();
    await row('english-q8.gguf').locator('progress').waitFor();
    await row('english-q8.gguf').getByRole('button',{name:'Cancel English Q8',exact:true}).click();
    await row('english-q8.gguf').locator('progress').waitFor({state:'detached'});
    assert(f.actions.some(action=>action.action==='cancel_model_download'&&action.filename==='english-q8.gguf'));
    assert.equal(f.models.find(model=>model.file==='english-q8.gguf').installed,false,'cancelled speech download never becomes installed');
    whisperInstalled=true; await refresh();
    await row('whisper-tiny').waitFor();
    await section.getByRole('button',{name:'Whisper',exact:true}).click();
    assert.equal(await row('whisper-tiny').count(),1); assert.equal(await row('english-q8.gguf').count(),0,'installed runtime engine pill scopes models');
    await section.getByRole('button',{name:'All engines',exact:true}).click();
    f.installFixture('paddle-ar-en'); await refresh();
    await choose('ocr','model');
    for(const query of ['paddle','installed Arabic','Arabic installed','عَرَبِي مُثَبَّت']) {
      await search.fill(query); assert.equal(await row('paddle-ar-en').count(),1,query);
    }
    await search.fill('');
    await section.locator('.installed-filter input').check();
    await choose('stt','model'); assert.equal(await section.locator('.installed-filter input').isChecked(),false,'installed filter is scoped');
    await page.locator('[data-page=audio]').click();
    const input=page.locator('[data-setting-path="stt.en.runtime"] .combo-input');await input.click();await input.fill('Vulkan');
    await page.locator('[data-combo-list="stt.en.runtime"]').getByRole('option',{name:'GPU',exact:true}).filter({hasText:'Parakeet Vulkan'}).click();
    assert.equal(await input.inputValue(),'GPU','selection uses human runtime label');
    await page.locator('[data-page=reading]').click();
    await page.locator('.module-download-link button').last().click();
    await page.locator('[data-catalog="ocr.runtime"]').waitFor();
    // Visual batch: both languages/themes, all six views and supported widths.
    f.options.recommendations.kokoro='nvidia_voice'; f.options.recommendations.supertonic3='multilingual_voice';
    f.installFixture('sherpa-cuda'); f.installFixture('english-q8.gguf');
    const output=path.resolve('artifacts/catalog-ui');fs.mkdirSync(output,{recursive:true});
    for(const locale of ['en','ar']) {
      await page.locator('[data-page=general]').click();await page.locator('#ui-language').selectOption(locale);
      for(const theme of ['light','dark']) {
        await page.locator('[data-setting-path="general.ui_theme"] select').selectOption(theme);
        for(const width of [360,768,1280]) {
          await page.setViewportSize({width,height:900});await page.locator('[data-page=models]').click();
          for(const feature of ['stt','tts','ocr']) for(const kind of ['runtime','model']) {
            await choose(feature,kind);await search.fill('');await section.locator('.installed-filter input').uncheck();
            const all=section.getByRole('button',{name:locale==='en'?'All engines':'كل المحركات',exact:true});if(await all.count())await all.click();
            await page.evaluate(()=>document.fonts.ready);
            assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),`${locale}/${theme}/${width}/${feature}/${kind} overflow`);
            const controls=await page.locator('.feature-catalogs button').evaluateAll(nodes=>nodes.map(node=>({width:node.clientWidth,scroll:node.scrollWidth})));assert(controls.every(node=>node.scroll<=node.width+2),'category labels fit');
            if(process.env.SETTINGS_VISUAL_REVIEW==='1')await page.screenshot({path:path.join(output,`${locale}-${theme}-${width}-${feature}-${kind}.png`),fullPage:true});
          }
          await page.locator('[data-page=general]').click();
        }
      }
    }
    assert.deepEqual(errors,[]);
    console.log('PASS six catalogs, runtime gating, orphan removal, human labels, recommendation search, independent downloads/cancel/queue, filters, contextual links and EN/AR layouts');
  } finally {await browser?.close();f.closeEvents();await new Promise(resolve=>f.server.close(resolve));}
})().catch(error=>{console.error(error.stack||error);process.exitCode=1;});
