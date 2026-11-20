<script lang="ts">
 import Combo from './Combo.svelte';
 import {supertonicVoices,matchesSupertonicVoice} from '../lib/voices';
 import {moduleSearchText} from '../lib/search';
 import {runtimeLabel} from '../lib/module-labels';
 import IconButton from './IconButton.svelte';
 import Help from './Help.svelte';
 import {modules,refreshModules,type Feature,type Binding} from '../lib/modules.svelte';
 import {t} from '../lib/i18n';
 import {notice} from '../lib/state.svelte';
 let {feature,binding,language='',path,onchange}:{feature:Feature;binding:Binding;language?:string;path:string;onchange:(binding:Binding)=>void}=$props();
 const runtime=$derived(modules.runtimes.find(r=>r.id===binding.runtime));
 const runtimes=$derived([{value:'',label:'None'},...modules.runtimes.filter(r=>r.feature===feature&&r.installed).sort((a,b)=>Number(!!b.recommended)-Number(!!a.recommended)||a.label.localeCompare(b.label)).map(r=>({value:r.id,label:runtimeLabel(r),meta:r.label,search:moduleSearchText(r,modules.models,modules.runtimes,modules.assets?.assets,false)}))]);
 const models=$derived([{value:'',label:'None'},...modules.models.filter(m=>m.feature===feature&&m.installed&&modules.runtimes.some(r=>r.feature===feature&&r.installed&&r.engine===m.engine)&&(!binding.runtime||(runtime?.installed&&m.engine===runtime.engine))&&(!language||!m.languages.length||m.languages.includes(language))).sort((a,b)=>Number(b.recommended)-Number(a.recommended)||a.label.localeCompare(b.label)).map(m=>({value:m.id,label:m.label,search:moduleSearchText(m,modules.models,modules.runtimes,modules.assets?.assets,false)}))]);
 async function refresh(){try{await refreshModules();}catch(error){notice(t('ui.operation_failed.c4e6ed'),'error',String(error));}}
 function changeRuntime(value:string){const r=modules.runtimes.find(r=>r.id===value);const model=modules.models.find(m=>m.id===binding.model);const clear=!!(r&&model&&r.engine!==model.engine);onchange({...binding,runtime:value||null,model:clear?null:binding.model,voice:clear?null:binding.voice,steps:clear?null:binding.steps,lowercase:clear?false:binding.lowercase,currency_words:binding.currency_words,clean_text:binding.clean_text,device:r?.backend==='cpu'?'cpu':binding.device});}
 const voices=$derived(binding.model==='supertonic3'?supertonicVoices():['af_heart','af_bella','af_nicole','af_sarah','am_adam','am_michael','bf_emma'].map(value=>({value,label:value})));
</script>
<div class="engine-assignment" data-module={feature} data-language={language||'single'}>
 <Combo label="module.runtime" path={`${path}.runtime`} value={binding.runtime||''} items={runtimes} unavailable onselect={changeRuntime}>
 {#snippet extra()}<IconButton icon="refresh" label="module.refresh" onclick={refresh}/>{/snippet}
 </Combo>
 {#if !modules.runtimes.some(r=>r.feature===feature&&r.installed)}<p class="inline-status">{t('module.install_runtime_first')}</p>{/if}
 <Combo label="ui.model.68c2cc" path={`${path}.model`} value={binding.model||''} items={models} unavailable onselect={(value)=>onchange({...binding,model:value||null,voice:value===binding.model?binding.voice:null,steps:value===binding.model?binding.steps:null,lowercase:value===binding.model?binding.lowercase:false,currency_words:binding.currency_words,clean_text:binding.clean_text})}>
 {#snippet extra()}<IconButton icon="refresh" label="module.refresh" onclick={refresh}/>{/snippet}
 </Combo>
 {#if feature==='tts' && (binding.model==='kokoro'||binding.model==='supertonic3')}
 <Combo label="reading.voice" path={`${path}.voice`} value={binding.voice||(binding.model==='kokoro'?'af_heart':'sid:0')} items={voices} searchMatcher={binding.model==='supertonic3'?matchesSupertonicVoice:undefined} onselect={(value)=>onchange({...binding,voice:value})}/>
 {#if binding.model==='kokoro'}
 <div class="field"><div class="field-copy"><label for={`${path}.lowercase`}>{t('module.lowercase')}</label><Help text="module.lowercase_help"/></div><div class="control"><input id={`${path}.lowercase`} type="checkbox" disabled={!(binding.clean_text??true)} checked={binding.lowercase??false} onchange={(e)=>onchange({...binding,lowercase:e.currentTarget.checked})}/></div></div>
 {/if}
 {#if binding.model==='supertonic3'}
 <div class="field"><div class="field-copy"><label for={`${path}.steps`}>{t('module.steps')} · {binding.steps??5}</label><Help text="module.steps_help"/></div><div class="control"><input id={`${path}.steps`} type="range" min="5" max="12" step="1" value={binding.steps??5} oninput={(e)=>onchange({...binding,steps:Number(e.currentTarget.value)})}/></div></div>
 {/if}
 {/if}
 <Combo label="ui.device.a5a74a" path={`${path}.device`} value={runtime?.backend==='cpu'?'cpu':binding.device} items={runtime?.backend==='cpu'?[{value:'cpu',label:'module.processor'}]:[{value:'auto',label:'ui.automatic.ac9041'},{value:'cpu',label:'module.processor'},{value:'gpu',label:runtime?.backend==='cuda'?'module.nvidia':'ui.gpu_backend'}]} onselect={value=>onchange({...binding,device:value as Binding['device']})}/>

</div>
