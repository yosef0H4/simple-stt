<script lang="ts">
import {onMount} from "svelte";
import {api} from "../lib/api";
import {reading,loadReading,saveReadingProvider,clearReaderKey} from "../lib/reading-state.svelte";
import {ui,notice,markDirty} from "../lib/state.svelte";
import {t} from "../lib/i18n";
import Group from "../components/Group.svelte";
import Combo from "./Combo.svelte";
import ProviderCredentials from "./ProviderCredentials.svelte";
import ProviderModels from "./ProviderModels.svelte";
import ModuleSelection from "../components/ModuleSelection.svelte";
let {feature}:{feature:"tts"|"ocr"}=$props();
let busy=$state(false);
let status=$state<Record<string,unknown>|null>(null);
$effect(()=>{markDirty();});
function refresh(){return loadReading().then(state=>{status=state.status;});}
async function clearKey(kind:"speech"|"ocr"){busy=true;try{await clearReaderKey(kind);markDirty();}catch(error){notice(t("reading.failed"),"error",String(error));}finally{busy=false;}}
async function saveConnection(kind:"speech"|"ocr"){busy=true;try{await saveReadingProvider(kind);markDirty();notice(t("ui.saved_and_applied.cc3757"),"success");}catch(error){notice(t("reading.failed"),"error",String(error));}finally{busy=false;}}

function route(section:"speech"|"ocr",value:string){
 if(!reading.config)return;
 if(section==="speech")reading.config.speech.provider=value==="local_kokoro"?{kind:"local_kokoro"}:{kind:"open_ai_compatible",base_url:"https://",model:"tts-1",voice:"alloy",format:"wav"};
 else reading.config.ocr.provider=value==="local_paddle"?{kind:"local_paddle"}:{kind:"open_ai_compatible",base_url:"https://",model:"vision-model",api:"chat_completions"};
}
onMount(()=>{void refresh().catch(error=>notice(t("reading.failed"),"error",String(error)));});
</script>
{#if reading.config}
  {#if feature === "tts"}<Group title="reading.speech" icon="audio">
    <p class="inline-status">{t("reading.configured_route")}: {t(reading.config.speech.provider.kind === "open_ai_compatible" ? "reading.route.remote" : "reading.route.local")}{#if typeof status?.speech_route_status === "string"} · {t("reading.active_route")}: {t(`reading.route.${String(status.speech_route_status)}`)}{/if}</p>
    <Combo label="settings.run_model" path="reading-provider" value={reading.config.speech.provider.kind === "local_supertonic" ? "local_kokoro" : reading.config.speech.provider.kind} items={[{value:"local_kokoro",label:"module.local_models"},{value:"open_ai_compatible",label:"ui.openai_compatible.b10d74"}]} onselect={value=>route("speech",value)}/>
    {#if reading.config.speech.provider.kind !== "open_ai_compatible"}
      <ModuleSelection feature="tts" showDownloads={false} />
    {:else}
      <div class="field"><label for="speech-base_url">{t('reading.base_url')}</label><div class="control"><input id="speech-base_url" dir="ltr" bind:value={reading.config.speech.provider.base_url}/></div></div>
      <ProviderCredentials id="speech-key" bind:value={reading.speechKey} saved={reading.speechKeySaved} {busy} onsave={()=>saveConnection("speech")} onclear={()=>clearKey("speech")}/>
      <ProviderModels feature="tts" baseUrl={reading.config.speech.provider.base_url} apiKey={reading.speechKey} model={reading.config.speech.provider.model} voice={reading.config.speech.provider.voice} onmodel={v=>{if(reading.config?.speech.provider.kind==='open_ai_compatible')reading.config.speech.provider.model=v;}} onvoice={v=>{if(reading.config?.speech.provider.kind==='open_ai_compatible')reading.config.speech.provider.voice=v;}}/>
      <div class="field"><label for="speech-format">{t('reading.format')}</label><div class="control"><input id="speech-format" bind:value={reading.config.speech.provider.format}/></div></div>
      <div class="field"><label for="speech-fallback">{t("reading.fallback_to_local")}</label><div class="control"><input id="speech-fallback" type="checkbox" role="switch" bind:checked={reading.config.speech.fallback_to_local} /></div></div>
      {#if reading.config.speech.fallback_to_local}<details class="guide"><summary>{t("settings.local_fallback")}</summary><ModuleSelection feature="tts" showDownloads={false}/></details>{/if}

    {/if}
  </Group>
  {:else}<Group title="reading.ocr" icon="models">
    <p class="inline-status">{t("reading.configured_route")}: {t(reading.config.ocr.provider.kind === "open_ai_compatible" ? "reading.route.remote" : "reading.route.local")}{#if typeof status?.ocr_route_status === "string"} · {t("reading.active_route")}: {t(`reading.route.${String(status.ocr_route_status)}`)}{/if}</p>
    <Combo label="settings.run_model" path="ocr-provider" value={reading.config.ocr.provider.kind} items={[{value:"local_paddle",label:"module.local_models"},{value:"open_ai_compatible",label:"ui.openai_compatible.b10d74"}]} onselect={value=>route("ocr",value)}/>
    {#if reading.config.ocr.provider.kind === "local_paddle"}
      <ModuleSelection feature="ocr" showDownloads={false} />
    {:else}
      <div class="field"><label for="ocr-base_url">{t('reading.base_url')}</label><div class="control"><input id="ocr-base_url" dir="ltr" bind:value={reading.config.ocr.provider.base_url}/></div></div>
      <ProviderCredentials id="ocr-key" bind:value={reading.ocrKey} saved={reading.ocrKeySaved} {busy} onsave={()=>saveConnection("ocr")} onclear={()=>clearKey("ocr")}/>
      <ProviderModels feature="ocr" baseUrl={reading.config.ocr.provider.base_url} apiKey={reading.ocrKey} model={reading.config.ocr.provider.model} onmodel={v=>{if(reading.config?.ocr.provider.kind==='open_ai_compatible')reading.config.ocr.provider.model=v;}}/>
      <div class="field"><label for="ocr-api">{t('reading.api')}</label><div class="control"><select id="ocr-api" bind:value={reading.config.ocr.provider.api}><option value="chat_completions">Chat Completions</option><option value="responses">Responses</option></select></div></div>
      <div class="field"><label for="ocr-fallback">{t("reading.fallback_to_local")}</label><div class="control"><input id="ocr-fallback" type="checkbox" role="switch" bind:checked={reading.config.ocr.fallback_to_local} /></div></div>
      {#if reading.config.ocr.fallback_to_local}<details class="guide"><summary>{t("settings.local_fallback")}</summary><ModuleSelection feature="ocr" showDownloads={false}/></details>{/if}
    {/if}
  </Group>

{/if}
{/if}
