<script lang="ts">
import {onMount} from 'svelte';
import {api} from '../lib/api';
import {save,ui,notice} from '../lib/state.svelte';
import {readingDirty} from '../lib/reading-state.svelte';
import {t} from '../lib/i18n';
import type {Connection} from '../lib/modules.svelte';
import ProviderCredentials from './ProviderCredentials.svelte';
import ProviderModels from './ProviderModels.svelte';
let {connection,feature,path,onchange}:{connection:Connection;feature:'stt'|'tts';path:string;onchange:(value:Connection)=>void}=$props();
let key=$state(''),saved=$state(false),busy=$state(false);
async function credentials(clear=false){
 busy=true;
 try{
  if(!clear){await save();if(ui.dirty||readingDirty())return;}
  const result=await api<{saved:boolean}>('/api/connection-secret',{slot:connection.key_slot,value:clear?undefined:key,clear});
  saved=result.saved;key='';
 }catch(error){notice(t('reading.failed'),'error',String(error));}finally{busy=false;}
}
onMount(()=>{if(connection.key_slot)void api<{saved:boolean}>('/api/connection-secret',{slot:connection.key_slot}).then(value=>saved=value.saved).catch(()=>{});});
</script>
<div class="field"><label for={`${path}-url`}>{t('reading.base_url')}</label><div class="control"><input id={`${path}-url`} dir="ltr" value={connection.base_url} oninput={event=>onchange({...connection,base_url:event.currentTarget.value})}/></div></div>
<ProviderCredentials id={`${path}-key`} bind:value={key} {saved} {busy} onsave={()=>credentials()} onclear={()=>credentials(true)}/>
<ProviderModels {feature} prefix={path} baseUrl={connection.base_url} apiKey={key} keySlot={connection.key_slot} model={connection.model} voice={connection.voice} onmodel={model=>onchange({...connection,model})} onvoice={feature==='tts'?voice=>onchange({...connection,voice}):undefined}/>
