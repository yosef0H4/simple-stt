<script lang="ts">
  import { ui, download, action, refreshState } from "../lib/state.svelte";
  import { usages } from "../lib/models";
  import IconButton from "../components/IconButton.svelte";
  import Icon from "../components/Icon.svelte";
  let query = $state(""),
    limit = $state(8);
  const models = $derived(
    (ui.state?.models || []).filter((m) =>
      `${m.family} ${m.file} ${m.quant} ${m.languages.join(" ")} ${m.installed ? "installed downloaded local" : ""} ${m.recommended ? "recommended" : ""}`
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
      aria-label="Find a model"
      placeholder="Search models"
      bind:value={query}
      oninput={() => (limit = 8)}
    />
  </div>
  <IconButton
    icon="refresh"
    label="Refresh model catalog"
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
          >{model.quant} · {model.size_mb ?? "—"} MB · {model.languages.join(
            ", ",
          )}</span
        ><small title={model.file}>{model.file}</small>
      </div>
      <div class="model-actions">
        {#if model.recommended}<span
            class="status-mark"
            title="Recommended"
            aria-label="Recommended"><Icon name="check" size={15} /></span
          >{/if}
        {#if progress}<IconButton
            icon="download"
            label={`Downloading ${model.family}`}
            disabled
          /><progress
            class="download-progress"
            value={progress.total
              ? (progress.downloaded / progress.total) * 100
              : undefined}
            max="100"
            aria-label={`Downloading ${model.family}`}
          ></progress>
        {:else if model.installed}<span
            class="status-mark installed"
            aria-label="Installed"
            title="Installed"><Icon name="check" size={16} /></span
          ><IconButton
            icon="trash"
            label={`Remove ${model.family} ${model.quant}`}
            title={used.length
              ? `Used by ${used.join(", ")}`
              : `Remove ${model.file}`}
            disabled={used.length > 0 || !ui.state?.service_online}
            onclick={async () => {
              await action("remove_model", model.file);
              await refreshState();
            }}
          />
        {:else}<IconButton
            icon="download"
            label={`Download ${model.family}`}
            disabled={!ui.state?.service_online}
            onclick={() => download(model.file)}
          />{/if}
      </div>
    </article>
  {:else}<p class="empty">No models match</p>{/each}
</div>
{#if models.length > limit}<button
    type="button"
    class="quiet"
    onclick={() => (limit += 8)}>View more</button
  >{/if}
