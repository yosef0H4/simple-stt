<script lang="ts">
  import { languageName, t } from "../lib/i18n";
  import { ui, download, action, refreshState } from "../lib/state.svelte";
  import { usages } from "../lib/models";
  import IconButton from "../components/IconButton.svelte";
  import Icon from "../components/Icon.svelte";
  let query = $state(""),
    limit = $state(8);
  const models = $derived(
    (ui.state?.models || []).filter((m) =>
      `${m.family} ${m.file} ${m.quant} ${m.languages.map(languageName).join(" ")} ${m.installed ? `${t("ui.installed.7bb440")} installed downloaded local مثبت` : ""} ${m.recommended ? `${t("ui.recommended.9ef937")} recommended موصى به` : ""}`
        .toLowerCase()
        .includes(query.toLowerCase()),
    ),
  );
</script>

<div class="catalog-toolbar">
  <div class="search-input">
    <Icon name="search" /><input
      id="model-search"
      type="search"
      aria-label={t("ui.find_a_model.2f629b")}
      placeholder={t("ui.search_models.5f018f")}
      bind:value={query}
      oninput={() => (limit = 8)}
    />
  </div>
  <IconButton
    icon="refresh"
    label={t("ui.refresh_model_catalog.d1077c")}
    disabled={!ui.state?.service_online}
    onclick={async () => {
      await action("refresh_models");
      await refreshState();
    }}
  />
</div>
<div id="model-results" class="model-results">
  {#each models.slice(0, limit) as model (model.file)}
    {@const used = usages(
      ui.state?.config || null,
      model.file,
      ui.state?.keyboard_languages?.languages || [],
    )}{@const progress = ui.downloads[model.file]}
    <article class="model-result">
      <div class="model-copy">
        <strong>{model.family}</strong><span
          >{model.quant} · {model.size_mb ?? "—"} MB · {model.languages.map(languageName).join(
            ", ",
          )}</span
        ><small title={model.file}>{model.file}</small>
      </div>
      <div class="model-actions">
        {#if model.recommended}<span
            class="status-mark"
            title={t("ui.recommended.9ef937")}
            aria-label={t("ui.recommended.9ef937")}><Icon name="check" size={15} /></span
          >{/if}
        {#if progress}<IconButton
            icon="download"
            label={t("ui.downloading_model.d1d3d0", { model: model.family })}
            disabled
          /><progress
            class="download-progress"
            value={progress.total
              ? (progress.downloaded / progress.total) * 100
              : undefined}
            max="100"
            aria-label={t("ui.downloading_model.d1d3d0", { model: model.family })}
          ></progress>
        {:else if model.installed}<span
            class="status-mark installed"
            aria-label={t("ui.installed.7bb440")}
            title={t("ui.installed.7bb440")}><Icon name="check" size={16} /></span
          ><IconButton
            icon="trash"
          label={t("ui.remove_model_quant.a7ef0e", { model: model.family, quant: model.quant })}
            title={used.length
              ? t("ui.used_by_items.225a64", { items: used.map(languageName).join(", ") })
              : t("ui.remove_file.f0834f", { file: model.file })}
            disabled={used.length > 0 || !ui.state?.service_online}
            onclick={async () => {
              await action("remove_model", model.file);
              await refreshState();
            }}
          />
        {:else}<IconButton
            icon="download"
            label={t("ui.download_model.16005c", { model: model.family })}
            disabled={!ui.state?.service_online}
            onclick={() => download(model.file)}
          />{/if}
      </div>
    </article>
  {:else}<p class="empty">{t("ui.no_models_match.53ffca")}</p>{/each}
</div>
{#if models.length > limit}<button
    type="button"
    class="quiet"
    onclick={() => (limit += 8)}>{t("ui.view_more.e3c5fa")}</button
  >{/if}
