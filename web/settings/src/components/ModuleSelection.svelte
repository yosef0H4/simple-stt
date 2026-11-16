<script lang="ts">
 import {onMount,tick} from 'svelte';
 import {t,languageName} from '../lib/i18n';
 import Combo from './Combo.svelte';
 import {ui,set,refreshLanguages,notice} from '../lib/state.svelte';
 import {reading} from '../lib/reading-state.svelte';
 import {modules,refreshModules,emptyBinding,catalogView,selectionView,type Binding,type Feature,type Selection} from '../lib/modules.svelte';
 import EngineAssignment from './EngineAssignment.svelte';
 import IconButton from './IconButton.svelte';
 let {feature,showDownloads=true}:{feature:Feature;showDownloads?:boolean}=$props();
 let adding=$state(false);
 const readerSection=$derived(feature==='ocr'?'ocr':'speech');
 const mode=$derived(feature==='stt'?ui.config?.speech.selection_mode:reading.config?.[readerSection].selection.mode);
 const languages=$derived(ui.state?.keyboard_languages?.languages||[]);
 const supported=$derived([...new Set(modules.models.filter(model=>model.feature==='tts').flatMap(model=>model.languages))].sort());
 const configured=$derived(Object.keys(selection()?.languages||{}).sort());
 const choices=$derived(mode==='auto_detect'
  ? [...configured.map(code=>({value:code,label:languageName(code)}))]
  : languages.map(language=>({value:language.id,label:languageName(language.id)})));
 const editing=$derived(choices.some(item=>item.value===selectionView[feature])?selectionView[feature]:(choices.find(item=>item.value==='en')?.value||choices[0]?.value||''));
 const pickingLanguage=$derived(mode==='auto_detect'&&feature==='tts'&&(adding||!choices.length));
 const addable=$derived(supported.filter(code=>!configured.includes(code)).map(code=>({value:code,label:languageName(code)})));
 function addLanguage(code:string){const s=selection();if(s&&code&&!s.languages[code]){s.languages[code]={...emptyBinding(),clean_text:s.single.clean_text,currency_words:s.single.currency_words,speed_percent:s.single.speed_percent};selectionView[feature]=code;adding=false;}}
 async function startAdding(){adding=true;await tick();document.getElementById('combo-tts.add-language')?.focus();}
 function removeLanguage(code:string){const s=selection();if(s){delete s.languages[code];selectionView[feature]='';}}
 function selection():Selection|undefined{return reading.config?.[readerSection].selection;}
 function binding(language:string):Binding {
  if(feature==='stt') {const s=ui.config?.speech;const runtimePath=s?.runtime_dir||'';return {runtime:language?(s?.language_runtimes?.[language]??null):(runtimePath.split(/[\\/]/).pop()||null),model:language?s?.language_models[language]||null:s?.single_model_filename||null,device:language?s?.language_devices?.[language]||'auto':s?.inference_device||'auto'};}
  const s=selection();return (language?s?.languages[language]:s?.single)||emptyBinding();
 }
 function update(value:Binding,language:string){
  if(feature==='stt'&&ui.config){const s=ui.config.speech;if(language){set(`speech.language_models.${language}`,value.model);s.language_runtimes??={};s.language_devices??={};s.language_runtimes[language]=value.runtime;s.language_devices[language]=value.device;set('speech.language_runtimes',{...s.language_runtimes});set('speech.language_devices',{...s.language_devices});}else{set('speech.single_model_filename',value.model);set('speech.runtime_dir',value.runtime?modulesRuntimeDirectory(value.runtime):'');set('speech.inference_device',value.device);}}
  else {const s=selection();if(s){if(language)s.languages[language]=value;else s.single=value;}}
 }
 function modulesRuntimeDirectory(id:string){return modules.runtimes.find(r=>r.id===id)?.directory||'';}
 function changeMode(value:string){adding=false;if(feature==='stt')set('speech.selection_mode',value);else {const s=selection();if(s)s.mode=value as Selection['mode'];}}
 onMount(()=>{void refreshModules().catch(e=>notice(t('ui.operation_failed.c4e6ed'),'error',String(e)));});
</script>
<div class="field"><label for={`${feature}-mode`}>{t('ui.mode.a7b93d')}</label><div class="control"><select id={`${feature}-mode`} value={mode} onchange={(e)=>changeMode(e.currentTarget.value)}><option value="single_model">{t('ui.use_one_model.bce8d2')}</option>{#if feature==='tts'}<option value="auto_detect">{t('module.auto_detect')}</option>{/if}<option value="follow_keyboard">{t('ui.follow_keyboard.565bed')}</option></select>{#if mode==='follow_keyboard'}<IconButton icon="refresh" label="ui.refresh_keyboard_languages.49fbe8" onclick={refreshLanguages}/>{/if}</div></div>
<svelte:window onkeydown={(event)=>{if(event.key==='Escape')adding=false;}}/>
<svelte:document onpointerdown={(event)=>{if(adding&&event.target instanceof Element&&!event.target.closest(`[data-language-picker="${feature}"]`))adding=false;}}/>
{#if mode!=='single_model'}
 {#if choices.length || (mode==='auto_detect' && feature==='tts')}
  <div data-language-picker={feature}>
   <Combo label="module.edit_language" ariaLabel={pickingLanguage?'module.add_language':undefined} path={pickingLanguage?'tts.add-language':`${feature}.edit-language`} value={pickingLanguage?'':editing} items={pickingLanguage?addable:choices} onselect={(code)=>{if(pickingLanguage)addLanguage(code);else selectionView[feature]=code;}}>
    {#snippet extra()}
     {#if mode==='auto_detect'&&feature==='tts'}<IconButton icon={adding&&choices.length?'close':'plus'} label={adding&&choices.length?'module.cancel_add_language':'module.add_language'} disabled={!addable.length&&!adding} onclick={()=>{if(adding&&choices.length)adding=false;else return startAdding();}}/>{/if}
    {/snippet}
   </Combo>
  </div>
  {#if !choices.length}<p class="inline-status">{t('module.empty_languages')}</p>{/if}
 {:else}<p class="inline-status">{t('ui.keyboard_detection_unavailable.091ebb')}</p>{/if}
{/if}
{#if mode==='single_model' || editing}
 <EngineAssignment {feature} showLanguageName={false} language={mode==='single_model'?'':editing} path={`${feature}.${mode==='single_model'?'single':editing}`} binding={binding(mode==='single_model'?'':editing)} onchange={(value)=>update(value,mode==='single_model'?'':editing)}/>
{/if}
{#if mode==='auto_detect' && feature==='tts'}
 {#if editing}<button type="button" class="quiet" aria-label={`${t('module.remove_language')} ${languageName(editing)}`} onclick={()=>removeLanguage(editing)}>{t('module.remove_language')}</button>{/if}

{/if}
{#if showDownloads}<p class="module-download-link"><button type="button" class="quiet" onclick={()=>{catalogView.feature=feature;catalogView.panel='models';catalogView.kind='runtime';ui.page='models';}}>{t('module.find_downloads')}</button></p>{/if}
