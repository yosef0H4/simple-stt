<script lang="ts">
  import { ui, action } from "../lib/state.svelte";
  import { fields } from "../lib/settings";
  import Group from "../components/Group.svelte";
  import Field from "../components/Field.svelte";
  import IconButton from "../components/IconButton.svelte";
</script>

<Group
  title="Runtime"
  paths={[
    "speech.runtime_dir",
    "speech.model_dir",
    "speech.idle_worker_timeout_secs",
    "speech.worker_shutdown_grace_ms",
  ]}
>
  {#snippet actions()}<IconButton
      icon="close"
      label="Unload speech model"
      disabled={!ui.state?.service_online}
      onclick={() => action("unload_model")}
    />{/snippet}
  {#each ["runtime", "modelDir", "idle", "grace"] as key}<Field
      spec={fields[key]}
    />{/each}
  <details class="guide">
    <summary>Resolved locations</summary><code
      >{ui.state?.resolved_runtime_dir || "Unavailable"}</code
    ><code>{ui.state?.resolved_model_dir || "Unavailable"}</code>
  </details>
</Group>
<Group
  title="Diagnostics"
  paths={[
    "diagnostics.log_level",
    "diagnostics.diagnostic_overlay",
    "diagnostics.log_transcripts",
  ]}
  >{#each ["logs", "overlay", "transcripts"] as key}<Field
      spec={fields[key]}
    />{/each}</Group
>
