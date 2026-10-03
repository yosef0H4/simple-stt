<script lang="ts">
  import { t } from "../lib/i18n";
  import { ui, action } from "../lib/state.svelte";
  import { fields } from "../lib/settings";
  import Group from "../components/Group.svelte";
  import Field from "../components/Field.svelte";
  import IconButton from "../components/IconButton.svelte";
</script>

<Group
  title="ui.runtime.c4740e"
  icon="advanced"
  paths={[
    "speech.runtime_dir",
    "speech.model_dir",
    "speech.idle_worker_timeout_secs",
    "speech.worker_shutdown_grace_ms",
  ]}
>
  {#snippet actions()}<IconButton
      icon="close"
      label="ui.unload_speech_model.045291"
      disabled={!ui.state?.service_online}
      onclick={() => action("unload_model")}
    />{/snippet}
  {#each ["runtime", "modelDir", "idle", "grace"] as key}<Field
      spec={fields[key]}
    />{/each}
  <details class="guide">
    <summary>{t("ui.resolved_locations.59a9d0")}</summary><code
      >{ui.state?.resolved_runtime_dir || t("ui.unavailable.2c9c1f")}</code
    ><code>{ui.state?.resolved_model_dir || t("ui.unavailable.2c9c1f")}</code>
  </details>
</Group>
<Group
  title="ui.diagnostics.3af227"
  icon="diagnostics"
  paths={[
    "diagnostics.log_level",
    "diagnostics.diagnostic_overlay",
    "diagnostics.log_transcripts",
  ]}
  >{#each ["logs", "overlay", "transcripts"] as key}<Field
      spec={fields[key]}
    />{/each}</Group
>
