<script lang="ts">
import {onMount} from "svelte";
import {ui,set,notice,saveSttProvider,markDirty} from "../lib/state.svelte";
import {api} from "../lib/api";
import {reading,loadReading} from "../lib/reading-state.svelte";
import {t} from "../lib/i18n";
import Combo from "./Combo.svelte";
import ProviderCredentials from "./ProviderCredentials.svelte";
import ProviderModels from "./ProviderModels.svelte";
import Group from "./Group.svelte";
import ModuleSelection from "./ModuleSelection.svelte";
let key=$state(""), busy=$state(false);
onMount(()=>{void loadReading().catch(()=>{});});
async function saveKey(clear=false){busy=true;try{if(clear){await api("/api/reader-secrets",{clear_stt:true});reading.sttKeySaved=false;}else{await saveSttProvider(key);if(key.trim())reading.sttKeySaved=true;notice(t("ui.saved_and_applied.cc3757"),"success");}key="";markDirty();}catch(error){notice(t("reading.failed"),"error",String(error));}finally{busy=false;}}

</script>
{#if ui.config}
<Group title="module.stt" icon="recognition">
 <Combo label="settings.run_model" path="speech.provider" value={ui.config.speech.provider??"local"} items={[{value:"local",label:"module.local_models"},{value:"remote_openai_compatible",label:"ui.openai_compatible.b10d74"}]} onselect={value=>set("speech.provider",value)}/>
 {#if ui.config.speech.provider==='remote_openai_compatible'}
  {#each [["base_url","reading.base_url"]] as [key,label]}<div class="field" data-setting-path={`speech.remote_openai_compatible.${key}`}><label for={`stt-${key}`}>{t(label)}</label><div class="control"><input id={`stt-${key}`} dir="ltr" value={ui.config.speech.remote_openai_compatible[key as "base_url"]} oninput={event=>set(`speech.remote_openai_compatible.${key}`,event.currentTarget.value)}/></div></div>{/each}
  <ProviderCredentials id="stt-key" bind:value={key} saved={reading.sttKeySaved} {busy} onsave={()=>saveKey()} onclear={()=>saveKey(true)}/>
  <ProviderModels feature="stt" baseUrl={ui.config.speech.remote_openai_compatible.base_url} apiKey={key} model={ui.config.speech.remote_openai_compatible.model} onmodel={v=>set('speech.remote_openai_compatible.model',v)}/>
  <div class="field"><label for="stt-fallback">{t("reading.fallback_to_local")}</label><div class="control"><input id="stt-fallback" type="checkbox" role="switch" checked={ui.config.speech.remote_openai_compatible.fallback_to_local} onchange={event=>set("speech.remote_openai_compatible.fallback_to_local",event.currentTarget.checked)}/></div></div>
  {#if ui.config.speech.remote_openai_compatible.fallback_to_local}<details class="guide"><summary>{t("settings.local_fallback")}</summary><ModuleSelection feature="stt" showDownloads={false}/></details>{/if}
 {:else}<ModuleSelection feature="stt" showDownloads={false}/>{/if}
</Group>
{/if}
