<script lang="ts">
  import {reading,loadReading} from "../lib/reading-state.svelte";
  import {onMount} from "svelte";
  import ReaderAdmin from "../components/ReaderAdmin.svelte";
  import Config from "./Config.svelte";
  onMount(()=>{void loadReading().catch(()=>{});});
  import { t } from "../lib/i18n";
  import { ui, action } from "../lib/state.svelte";
  import { fields } from "../lib/settings";
  import Group from "../components/Group.svelte";
  import Field from "../components/Field.svelte";
  import IconButton from "../components/IconButton.svelte";
</script>

<Group title="settings.memory" icon="advanced" paths={["speech.idle_worker_timeout_secs","speech.worker_shutdown_grace_ms"]}>
 <Field spec={fields.idle}/><Field spec={fields.grace}/>
 {#if reading.config}
 <div class="field"><label for="reading-idle">{t("reading.idle_timeout")}</label><div class="control"><input id="reading-idle" type="range" min="0" max="3600" step="1" bind:value={reading.config.playback.keep_warm_seconds}/><output class="range-value" for="reading-idle">{reading.config.playback.keep_warm_seconds}s</output></div></div>
 <div class="field"><label for="ocr-idle">{t("module.ocr_idle")}</label><div class="control"><input id="ocr-idle" type="range" min="0" max="3600" step="1" bind:value={reading.config.ocr.idle_timeout_secs}/><output class="range-value" for="ocr-idle">{reading.config.ocr.idle_timeout_secs}s</output></div></div>
 {/if}
</Group>
<ReaderAdmin/>
<Group
  title="ui.diagnostics.3af227"
  icon="diagnostics"
  paths={[
    "diagnostics.log_level",
    "diagnostics.diagnostic_overlay",
    "diagnostics.log_transcripts",
    ...(ui.state?.platform === "linux"
      ? ["diagnostics.clipboard_buffer_notices"]
      : []),
  ]}
  >{#each ["logs", "overlay", "transcripts"] as key}<Field
      spec={fields[key]}
    />{/each}{#if ui.state?.platform === "linux"}<Field
      spec={fields.clipboardBufferNotices}
    />{/if}</Group
>

<details class="guide" data-settings-json><summary>{t("settings.configuration")}</summary><Config/></details>
