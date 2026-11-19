<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "../lib/api";
  import { t } from "../lib/i18n";
  import { reading, loadReading } from "../lib/reading-state.svelte";
  import { ui, markDirty, notice } from "../lib/state.svelte";
  import ActionRecorder from "../components/ActionRecorder.svelte";
  import ReadingBehavior from "../components/ReadingBehavior.svelte";
  import Range from "../components/Range.svelte";
  import SelectionCopy from "../components/SelectionCopy.svelte";
  import Group from "../components/Group.svelte";
  import type { ReaderConfig } from "../lib/reader";
  let text = $state("");
  let serverToken = $state("");
  let speechKeySaved = $state(false); let ocrKeySaved = $state(false);
  let busy = $state(false);
  let status = $state<Record<string, unknown> | null>(null);

  $effect(() => {markDirty();});
  async function showToken() {
    try {serverToken=(await api<{token:string}>("/api/reader-server-key",{})).token;}
    catch(error) {notice(t("reading.failed"),"error",String(error));}
  }
  async function clearKey(kind: "speech" | "ocr") {
    try {await api("/api/reader-secrets",{[`clear_${kind}`]:true});await refresh();}
    catch(error) {notice(t("reading.failed"),"error",String(error));}
  }
  async function refresh() {
    const state = await loadReading();
    status = state.status;
  }
  async function action(action: string) {
    busy = true;
    try {await api("/api/reader-action",{action,text}); await refresh();}
    catch(error) {notice(t("reading.failed"),"error",String(error));}
    finally {busy=false;}
  }
  function route(section: "speech"|"ocr", value: string) {
    if (!reading.config) return;
    if (section === "speech") reading.config.speech.provider = value === "local_kokoro" ? {kind:"local_kokoro"} : {kind:"open_ai_compatible",base_url:"https://",model:"tts-1",voice:"alloy",format:"wav"};
    else reading.config.ocr.provider = value === "local_paddle" ? {kind:"local_paddle"} : {kind:"open_ai_compatible",base_url:"https://",model:"vision-model",api:"chat_completions"};
  }
  onMount(() => {
    let disposed=false;
    void refresh().catch(error=>notice(t("reading.failed"),"error",String(error)));
    const timer=setInterval(()=>{if(!disposed) void api<{status: Record<string,unknown>|null}>("/api/reader-status").then(value=>status=value.status).catch(()=>{});},1500);
    return ()=>{disposed=true;clearInterval(timer);};
  });
</script>

{#if reading.config}

  <Group title="reading.controls" icon="audio">
    <textarea rows="3" dir="auto" bind:value={text} aria-label={t("reading.text")} placeholder={t("reading.text")}></textarea>
    <div class="reading-actions">
      {#each ["read_text","read_region","copy_region","stop"] as name}
        <button type="button" disabled={busy || (name === "read_text" && !text.trim())} onclick={()=>action(name)}>{t(`reading.${name}`)}</button>
      {/each}
      <button type="button" class="quiet" disabled={busy} onclick={()=>action("read_along")}>{t("reading.read_along_start")}</button>
    </div>
    <details class="guide"><summary>{t("reading.more")}</summary><div class="reading-actions">{#each ["read_selection","read_clipboard","listen_clipboard","pause","resume","previous","replay","next","unload"] as name}<button type="button" disabled={busy} onclick={()=>action(name)}>{t(`reading.${name}`)}</button>{/each}</div></details>
    <p role="status">{status ? t(`reading.${String(status.state)}`) : t("reading.offline")}{#if status?.clipboard_listening} · {t("reading.listening")}{/if}</p>
    {#if typeof status?.prepared_text === 'string' && status.prepared_text}<div class="spoken-text"><strong>{t('reading.spoken_text')}</strong><p dir="auto">{status.prepared_text}</p></div>{/if}
  </Group>
  <Group title="reading.read_along" icon="keyboard">
    <div class="field"><label for="read-along-action">{t("reading.read_along_action")}</label><div class="control"><select id="read-along-action" bind:value={reading.config.read_along.action}><option value="key">{t("reading.read_along_key")}</option><option value="click">{t("reading.read_along_click")}</option></select></div></div>
    {#if reading.config.read_along.action === "key"}<ActionRecorder />{/if}
    <div class="field"><label for="read-along-prefetch">{t("reading.read_along_prefetch")}</label><div class="control"><input id="read-along-prefetch" type="checkbox" role="switch" bind:checked={reading.config.read_along.prefetch} /></div></div>
    <Range label="reading.read_along_delay" path="read_along.delay_ms" value={reading.config.read_along.delay_ms} min={0} max={10000} step={50} unit="ms" onchange={value=>{if(reading.config)reading.config.read_along.delay_ms=value;}}/>
    <Range label="reading.read_along_retries" path="read_along.retry_count" value={reading.config.read_along.retry_count} min={0} max={5} step={1} unit="" onchange={value=>{if(reading.config)reading.config.read_along.retry_count=value;}}/>
  </Group>
  <SelectionCopy />
  <ReadingBehavior />

{/if}
<style>
  textarea {width:100%;box-sizing:border-box;}
  .reading-actions {display:flex;flex-wrap:wrap;gap:.5rem;margin-block:1rem;}
  p {color:var(--muted);line-height:1.6;}
</style>
