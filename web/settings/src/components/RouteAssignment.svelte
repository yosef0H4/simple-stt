<script lang="ts">
import {t} from '../lib/i18n';
import {markDirty} from '../lib/state.svelte';
import type {Binding,Feature,RemoteRoute,Connection} from '../lib/modules.svelte';
import EngineAssignment from './EngineAssignment.svelte';
import ConnectionEditor from './ConnectionEditor.svelte';
let {feature,language,path,binding,onchange}:{feature:Feature;language:string;path:string;binding:Binding;onchange:(value:Binding)=>void}=$props();
function connection(role:string):Connection{return {base_url:'',model:'',voice:'',format:'wav',timeout_ms:60000,key_slot:`${path}.${role}`};}
const remote=$derived(binding.remote??{enabled:false,primary:connection('primary'),fallback:null,fallback_to_local:false});
function update(value:RemoteRoute){onchange({...binding,remote:value});markDirty();}
</script>
{#if feature==='ocr'||(feature==='stt'&&!language)}
 <EngineAssignment {feature} {language} {path} {binding} showLanguageName={false} {onchange}/>
{:else}
 <div class="field"><label for={`${path}-route`}>{t('settings.language_route')}</label><div class="control"><select id={`${path}-route`} value={binding.remote?(remote.enabled?'remote':'local'):'default'} onchange={event=>{if(event.currentTarget.value==='default'){onchange({...binding,remote:null});markDirty();}else update({...remote,enabled:event.currentTarget.value==='remote'});}}><option value="default">{t('settings.default_route')}</option><option value="local">{t('module.local_models')}</option><option value="remote">{t('ui.openai_compatible.b10d74')}</option></select></div></div>
 {#if remote.enabled}
  <ConnectionEditor {feature} path={`${path}.primary`} connection={remote.primary} onchange={primary=>update({...remote,primary})}/>
  <div class="field"><label for={`${path}-server-fallback`}>{t('settings.fallback_server')}</label><div class="control"><input id={`${path}-server-fallback`} type="checkbox" role="switch" checked={!!remote.fallback} onchange={event=>update({...remote,fallback:event.currentTarget.checked?connection('fallback'):null})}/></div></div>
  {#if remote.fallback}<ConnectionEditor {feature} path={`${path}.fallback`} connection={remote.fallback} onchange={fallback=>update({...remote,fallback})}/>{/if}
  <div class="field"><label for={`${path}-local-fallback`}>{t('reading.fallback_to_local')}</label><div class="control"><input id={`${path}-local-fallback`} type="checkbox" role="switch" checked={remote.fallback_to_local} onchange={event=>update({...remote,fallback_to_local:event.currentTarget.checked})}/></div></div>
  {#if remote.fallback_to_local}<details class="guide"><summary>{t('settings.local_fallback')}</summary><EngineAssignment {feature} {language} {path} {binding} showLanguageName={false} {onchange}/></details>{/if}
 {:else}<EngineAssignment {feature} {language} {path} {binding} showLanguageName={false} {onchange}/>{/if}
{/if}
