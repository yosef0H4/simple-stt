<script lang="ts">
import {api} from '../lib/api';
import {t} from '../lib/i18n';
import Combo from './Combo.svelte';
let {feature,baseUrl='',apiKey='',model='',voice='',onmodel,onvoice}:{feature:'stt'|'tts'|'ocr';baseUrl?:string;apiKey?:string;model?:string;voice?:string;onmodel:(v:string)=>void;onvoice?:(v:string)=>void}=$props();
type Entry={id:string;label?:string;model?:string};
let models=$state<Entry[]>([]),voices=$state<Entry[]>([]),busy=$state(false),error=$state(''),loaded=$state(false),generation=0;
$effect(()=>{baseUrl;apiKey;generation++;models=[];voices=[];loaded=false;error='';});
async function refresh(){const revision=++generation;busy=true;error='';try{const result=await api<{models:Entry[];voices:Entry[]}>('/api/provider-discovery',{feature,base_url:baseUrl,api_key:apiKey||undefined,model});if(revision!==generation)return;models=result.models;voices=result.voices;loaded=true;}catch(e){if(revision===generation)error=String(e);}finally{busy=false;}}
</script>
<div class="reading-actions"><button type="button" disabled={busy||!baseUrl.trim()} aria-busy={busy} onclick={refresh}>{t('settings.fetch_provider')}</button><span>{t('settings.provider_manual')}</span></div>
{#if error}<p role="alert">{error}</p>{:else if loaded&&!models.length}<p role="status">{t('settings.provider_empty')}</p>{/if}
<Combo label="reading.model" path={`${feature}-remote-model`} value={model} items={models.map(m=>({value:m.id,label:m.label||m.id}))} custom onselect={onmodel}/>
{#if onvoice}<Combo label="reading.voice" path={`${feature}-remote-voice`} value={voice} items={voices.filter(v=>!v.model||v.model===model).map(v=>({value:v.id,label:v.label||v.id}))} custom onselect={onvoice}/>{/if}
