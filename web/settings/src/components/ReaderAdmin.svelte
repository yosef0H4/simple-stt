<script lang="ts">
import {onMount} from "svelte";
import {api} from "../lib/api";
import {reading,loadReading} from "../lib/reading-state.svelte";
import {notice,markDirty} from "../lib/state.svelte";
import {t} from "../lib/i18n";
import type {ReaderConfig} from "../lib/reader";
import Group from "./Group.svelte";
let serverToken=$state(""), busy=$state(false);
$effect(()=>{markDirty();});
onMount(()=>{void loadReading().catch(error=>notice(t("reading.failed"),"error",String(error)));});
async function showToken(){try{serverToken=(await api<{token:string}>("/api/reader-server-key",{})).token;}catch(error){notice(t("reading.failed"),"error",String(error));}}
async function action(action:string){busy=true;try{await api("/api/reader-action",{action});}catch(error){notice(t("reading.failed"),"error",String(error));}finally{busy=false;}}
</script>
{#if reading.config}
  <Group title="reading.server" icon="advanced">
    <div class="field"><label for="reader-server">{t("reading.enabled")}</label><div class="control"><input id="reader-server" type="checkbox" role="switch" bind:checked={reading.config.server.enabled} /></div></div>
    <div class="field"><label for="reader-listen">{t("reading.address")}</label><div class="control"><input id="reader-listen" dir="ltr" bind:value={reading.config.server.listen} /></div></div>
    <div class="reading-actions"><button type="button" disabled={busy} onclick={()=>action("server_start")}>{t("reading.start_server")}</button><button type="button" disabled={busy} onclick={()=>action("server_stop")}>{t("reading.stop_server")}</button></div>
  </Group>
  <div class="field"><span>{t("reading.server_token")}</span><div class="control"><button type="button" onclick={showToken}>{t("reading.show_token")}</button>{#if serverToken}<input aria-label={t("reading.server_token")} dir="ltr" readonly value={serverToken} />{/if}</div></div>
  <details class="guide"><summary>{t("reading.advanced")}</summary><p>{t("reading.advanced_help")}</p><textarea rows="16" dir="ltr" bind:value={reading.raw} aria-label={t("reading.advanced")}></textarea><button type="button" onclick={()=>{try {const next=JSON.parse(reading.raw) as ReaderConfig; if(!next.speech || !next.ocr || !next.shortcuts || !next.server)throw Error(t("reading.invalid"));reading.config=next;reading.rawError="";}catch(error){reading.rawError=String(error);}}}>{t("reading.apply")}</button>{#if reading.rawError}<p role="alert">{reading.rawError}</p>{/if}</details>
{/if}
