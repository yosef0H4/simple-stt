<script lang="ts">
  import {
    ui,
    preview,
    reset,
    platformAction,
    notice,
    editJson,
    load,
  } from "../lib/state.svelte";
  import IconButton from "../components/IconButton.svelte";
  import { t } from "../lib/i18n";
  let file: HTMLInputElement;
  $effect(() => {
    if (!ui.jsonPending) ui.jsonText = JSON.stringify(ui.config, null, 2);
  });
  async function importFile(event: Event) {
    const uploaded = (event.currentTarget as HTMLInputElement).files?.[0];
    if (uploaded)
      await preview(
        JSON.parse(await uploaded.text()),
        t("ui.import_previewed_save_to_write.1403f7"),
      );
    file.value = "";
  }
  function exportFile() {
    const blob = new Blob([JSON.stringify(ui.config, null, 2) + "\n"], {
      type: "application/json",
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "simple-stt-config.json";
    a.click();
    URL.revokeObjectURL(url);
  }
</script>

<div class="config-toolbar">
  <IconButton
    icon="config"
    label="ui.open_file.f11b87"
    onclick={() => platformAction("open_config")}
  /><IconButton
    icon="folder"
    label="ui.open_folder.f96301"
    onclick={() => platformAction("open_config_folder")}
  /><IconButton
    icon="refresh"
    label="ui.reload.cce715"
    onclick={async () => {
      await load();
    }}
  /><span class="toolbar-spacer"></span><IconButton
    icon="upload"
    label="ui.import.d6fbc9"
    onclick={() => file.click()}
  /><IconButton
    icon="download"
    label="ui.export.f3e4fa"
    onclick={exportFile}
  /><IconButton
    icon="copy"
    label="ui.copy_json.35db7b"
    onclick={() =>
      navigator.clipboard.writeText(JSON.stringify(ui.config, null, 2))}
  /><IconButton icon="reset" label="ui.reset_preview.830d3b" onclick={reset} />
</div>
<input
  id="import-file"
  bind:this={file}
  type="file"
  accept="application/json,.json"
  hidden
  onchange={(e) =>
    importFile(e).catch((error) => notice(String(error), "error"))}
/>
<label class="json-label" for="json">config.json</label><textarea
  id="json"
  class="json-editor"
  spellcheck="false"
  value={ui.jsonText}
  oninput={(event) => editJson(event.currentTarget.value)}></textarea>
