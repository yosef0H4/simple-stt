<script lang="ts">
import {api} from '../lib/api';
import {t} from '../lib/i18n';
import Combo from './Combo.svelte';
import IconButton from './IconButton.svelte';
let {feature,baseUrl='',apiKey='',model='',voice='',onmodel,onvoice}:{feature:'stt'|'tts'|'ocr';baseUrl?:string;apiKey?:string;model?:string;voice?:string;onmodel:(v:string)=>void;onvoice?:(v:string)=>void}=$props();
type Entry={id:string;label?:string;model?:string};
let models=$state<Entry[]>([]),voices=$state<Entry[]>([]),busy=$state(false),error=$state(''),loaded=$state(false),generation=0;
let controller:AbortController|undefined;
function validUrl(value:string){return /^https?:\/\//.test(value.trim())&&URL.canParse(value.trim());}
$effect(()=>{
  const url=baseUrl,key=apiKey,kind=feature;void key;void kind;
  generation++;controller?.abort();models=[];voices=[];loaded=false;error='';busy=false;
  if(!validUrl(url))return;
  const timer=setTimeout(()=>{void refresh();},600);
  return ()=>{clearTimeout(timer);generation++;controller?.abort();};
});
async function refresh(){
  controller?.abort();controller=new AbortController();
  const revision=++generation;busy=true;error='';
  try{
    const result=await api<{models:Entry[];voices:Entry[]}>('/api/provider-discovery',{feature,base_url:baseUrl,api_key:apiKey.trim()||undefined,model},controller.signal);
    if(revision!==generation)return;
    models=result.models;voices=result.voices;loaded=true;
  }catch(e){if(revision===generation)error=String(e);}
  finally{if(revision===generation)busy=false;}
}
</script>
{#if error}<p role="alert">{error}</p>{:else if loaded&&!models.length}<p role="status">{t('settings.provider_empty')}</p>{/if}
<Combo label="reading.model" path={`${feature}-remote-model`} value={model} items={models.map(m=>({value:m.id,label:m.label||m.id}))} custom onselect={onmodel}>
  {#snippet extra()}<IconButton icon="refresh" label="settings.refresh_models" disabled={busy||!validUrl(baseUrl)} onclick={refresh}/>{/snippet}
</Combo>
{#if onvoice}<Combo label="reading.voice" path={`${feature}-remote-voice`} value={voice} items={voices.filter(v=>!v.model||v.model===model).map(v=>({value:v.id,label:v.label||v.id}))} custom onselect={onvoice}>
  {#snippet extra()}<IconButton icon="refresh" label="settings.refresh_voices" disabled={busy||!validUrl(baseUrl)} onclick={refresh}/>{/snippet}
</Combo>{/if}
