<script lang="ts">
import {onMount} from "svelte";
import {ui,set,notice} from "../lib/state.svelte";
import {api} from "../lib/api";
import {loadReading} from "../lib/reading-state.svelte";
import {t} from "../lib/i18n";
import Combo from "./Combo.svelte";
import ProviderModels from "./ProviderModels.svelte";
import Group from "./Group.svelte";
import ModuleSelection from "./ModuleSelection.svelte";
let key=$state(""), saved=$state(false);
onMount(()=>{void loadReading().then(state=>saved=!!state.stt_key_saved).catch(()=>{});});
async function saveKey(clear=false){try{await api("/api/reader-secrets",clear?{clear_stt:true}:{stt:key});key="";saved=!clear;}catch(error){notice(t("reading.failed"),"error",String(error));}}
</script>
{#if ui.config}
<Group title="module.stt" icon="recognition">
 <Combo label="settings.run_model" path="speech.provider" value={ui.config.speech.provider??"local"} items={[{value:"local",label:"module.local_models"},{value:"remote_openai_compatible",label:"ui.openai_compatible.b10d74"}]} onselect={value=>set("speech.provider",value)}/>
 {#if ui.config.speech.provider==='remote_openai_compatible'}
  {#each [["base_url","reading.base_url"]] as [key,label]}<div class="field" data-setting-path={`speech.remote_openai_compatible.${key}`}><label for={`stt-${key}`}>{t(label)}</label><div class="control"><input id={`stt-${key}`} dir="ltr" value={ui.config.speech.remote_openai_compatible[key as "base_url"]} oninput={event=>set(`speech.remote_openai_compatible.${key}`,event.currentTarget.value)}/></div></div>{/each}
  <div class="field"><label for="stt-key">{t("reading.api_key")}</label><div class="control"><input id="stt-key" type="password" autocomplete="new-password" bind:value={key} placeholder={t(saved?"ui.replace_saved_key.893959":"ui.paste_api_key.7da909")}/><button type="button" disabled={!key.trim()} onclick={()=>saveKey()}>{t("ui.save_api_key.d8e28f")}</button><button type="button" disabled={!saved} onclick={()=>saveKey(true)}>{t("reading.clear_key")}</button></div></div>
  <ProviderModels feature="stt" baseUrl={ui.config.speech.remote_openai_compatible.base_url} apiKey={key} model={ui.config.speech.remote_openai_compatible.model} onmodel={v=>set('speech.remote_openai_compatible.model',v)}/>
  <div class="field"><label for="stt-fallback">{t("reading.fallback_to_local")}</label><div class="control"><input id="stt-fallback" type="checkbox" role="switch" checked={ui.config.speech.remote_openai_compatible.fallback_to_local} onchange={event=>set("speech.remote_openai_compatible.fallback_to_local",event.currentTarget.checked)}/></div></div>
  {#if ui.config.speech.remote_openai_compatible.fallback_to_local}<details class="guide"><summary>{t("settings.local_fallback")}</summary><ModuleSelection feature="stt" showDownloads={false}/></details>{/if}
 {:else}<ModuleSelection feature="stt" showDownloads={false}/>{/if}
</Group>
{/if}
