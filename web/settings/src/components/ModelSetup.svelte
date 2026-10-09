<script lang="ts">
  import { ui, download, run, refreshState } from "../lib/state.svelte";
  import { api } from "../lib/api";
  import { languageName, t } from "../lib/i18n";
  let english = $state(true), arabic = $state(true), busy = $state(false);
  const setup = $derived(ui.state?.model_setup);
  const choices = $derived([
    { file: setup?.english_file || "", language: "English (en)", checked: english },
    { file: setup?.arabic_file || "", language: "Arabic (ar)", checked: arabic },
  ]);
  async function finish(startDownloads: boolean) {
    if (busy) return;
    busy = true;
    try {
      if (startDownloads) {
        for (const choice of choices.filter((choice) => choice.checked)) {
          if (!ui.downloads[choice.file] && !ui.state?.models.find((model) => model.file === choice.file)?.installed)
            await download(choice.file);
        }
      }
      await api("/api/model-setup", { english: startDownloads && english, arabic: startDownloads && arabic });
      if (ui.state?.model_setup) ui.state.model_setup.show = false;
      await refreshState(true);
      if (startDownloads) ui.page = "models";
    } finally { busy = false; }
  }
</script>

<section id="recommended-setup" aria-labelledby="model-setup-title" aria-busy={busy}>
  <h2 id="model-setup-title">{t("ui.downloadRecommendedPrompt")}</h2>
  <div class="model-choices">
    <label><input type="checkbox" bind:checked={english} disabled={busy} />{languageName("English (en)")}</label>
    <label><input type="checkbox" bind:checked={arabic} disabled={busy} />{languageName("Arabic (ar)")}</label>
  </div>
  {#if !ui.state?.service_online}<p>{t("ui.startAppDownload")}</p>{/if}
  <div class="setup-actions">
    <button type="button" class="primary" disabled={busy || !ui.state?.service_online || (!english && !arabic)} onclick={() => run(() => finish(true))}>{t("ui.downloadSelected")}</button>
    <button type="button" class="quiet" disabled={busy} onclick={() => run(() => finish(false))}>{t("ui.skipModelSetup")}</button>
  </div>
</section>

<style>
  section { max-width: 840px; margin-inline: auto; border-block-end: 1px solid var(--line); padding-block: 0 20px; margin-block-end: 28px; }
  h2 { font-size: 16px; margin: 0 0 14px; }
  .model-choices { display: flex; flex-wrap: wrap; gap: 12px 24px; margin-block-end: 14px; }
  label { display: inline-flex; align-items: center; gap: 8px; }
  input { width: 16px; height: 16px; margin: 0; }
  .setup-actions { display: flex; flex-wrap: wrap; gap: 8px; }
  p { color: var(--muted); font-size: 13px; margin-block: 0 14px; }
</style>
