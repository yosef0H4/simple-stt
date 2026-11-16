<script lang="ts">
  import { reading } from "../lib/reading-state.svelte";
  import { ui, platformAction, notice } from "../lib/state.svelte";
  import { t } from "../lib/i18n";
  import Group from "./Group.svelte";
  import Combo from "./Combo.svelte";
  import IconButton from "./IconButton.svelte";

  const shortcuts = [
    { value: "ctrl_insert", label: "Ctrl+Insert" },
    { value: "ctrl_c", label: "Ctrl+C" },
    { value: "ctrl_shift_c", label: "Ctrl+Shift+C" },
  ];
  function add(appId = "") {
    if (!reading.config) return;
    reading.config.selection_copy_overrides.push({ app_id: appId, shortcut: reading.config.selection_copy.shortcut, backend: null });
  }
  async function addCurrent() {
    try {
      const result = await platformAction("focused_app");
      if (result.app_id && !reading.config?.selection_copy_overrides.some(entry => entry.app_id === result.app_id)) add(result.app_id);
    } catch (error) {
      notice(t("reading.failed"), "error", String(error));
    }
  }
</script>

{#if reading.config}
  <Group title="reading.copy_title" icon="text">
    <p class="copy-help">{t("reading.copy_help")}</p>
    <Combo label="reading.copy_shortcut" path="selection_copy.shortcut" value={reading.config.selection_copy.shortcut} items={shortcuts} onselect={value => { if (reading.config) reading.config.selection_copy.shortcut = value; }} />
    <div class="field"><label for="selection-clipboard">{t("reading.copy_allow_clipboard")}</label><div class="control"><input id="selection-clipboard" type="checkbox" role="switch" bind:checked={reading.config.selection_copy.allow_clipboard_copy} /></div></div>
    <div class="field"><label for="selection-notice">{t("reading.copy_show_notice")}</label><div class="control"><input id="selection-notice" type="checkbox" role="switch" bind:checked={reading.config.selection_copy.show_clipboard_notice} /></div></div>
  </Group>
  <Group title="reading.copy_overrides" icon="apps">
    {#snippet actions()}
      <IconButton icon="plus" label="ui.add_manually.a1d9c2" onclick={() => add()} />
      <IconButton icon="output" label="ui.add_current_app.4321a4" title="ui.add_current_app_focus_it.319c40" onclick={addCurrent} />
    {/snippet}
    <p class="copy-help">{t("ui.add_current_app_focus_it.319c40")}</p>
    {#each reading.config.selection_copy_overrides as entry, index}
      <div class="override">
        <div class="override-head"><label for={`copy-app-${index}`}>{t("ui.application_identity.74e9a6")}</label><IconButton icon="trash" label="ui.remove_app_override.10d229" onclick={() => reading.config?.selection_copy_overrides.splice(index, 1)} /></div>
        <input id={`copy-app-${index}`} dir="ltr" placeholder={t("ui.application_id.557031")} bind:value={entry.app_id} />
        <Combo label="reading.copy_shortcut" path={`selection_copy_overrides.${index}.shortcut`} value={entry.shortcut} items={shortcuts} onselect={value => entry.shortcut = value} />
      </div>
    {:else}<p class="copy-help">{t("ui.no_overrides.473cfd")}</p>{/each}
  </Group>
{/if}
<style>
  .copy-help { color: var(--muted); line-height: 1.5; }
  .override { padding-block: .75rem; border-block-start: 1px solid var(--border); min-width: 0; }
  .override-head { display: flex; align-items: center; justify-content: space-between; gap: .5rem; }
  .override > input { box-sizing: border-box; width: 100%; min-width: 0; }
</style>
