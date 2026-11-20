<script lang="ts">
  import { onMount } from 'svelte';
  import { matchesCatalogSearch, moduleSearchText } from '../lib/search';
  import { modules, refreshModules, catalogView, emptyBinding, type Feature } from '../lib/modules.svelte';
  import { ui, download, action, notice, markDirty } from '../lib/state.svelte';
  import { reading } from '../lib/reading-state.svelte';
  import type { ReaderConfig } from '../lib/reader';
  import SttModelSetup from "../components/SttModelSetup.svelte";
  import ReaderModelSetup from "../components/ReaderModelSetup.svelte";
  import Cleanup from "./Cleanup.svelte";
  import ModelFiles from "../components/ModelFiles.svelte";
  import {loadReading} from "../lib/reading-state.svelte";
  import Group from '../components/Group.svelte';
  import Help from '../components/Help.svelte';
  import { api } from '../lib/api';
  import { t, languageName } from '../lib/i18n';
  import { runtimeLabel, recommendationLabel } from '../lib/module-labels';
  import Icon from '../components/Icon.svelte';
  import IconButton from '../components/IconButton.svelte';

  type Kind = 'runtime' | 'model';
  type CatalogFilter = { query: string; engine: string; installed: boolean; limit: number };
  const features = [
    { id: 'stt', label: 'settings.dictation', acronym: 'STT', icon: 'audio' },
    { id: 'tts', label: 'module.speak', acronym: 'TTS', icon: 'text' },
    { id: 'ocr', label: 'module.vision', acronym: 'OCR', icon: 'screen' },
  ] as const;
  const kinds = [
    { id: 'runtime', label: 'module.runtimes' },
    { id: 'model', label: 'ui.models.f3798f' },
  ] as const;
  const newFilter = (): CatalogFilter => ({ query: '', engine: 'all', installed: false, limit: 30 });
  const filters = $state<Record<`${Feature}.${Kind}`, CatalogFilter>>({
    'stt.runtime': newFilter(), 'stt.model': newFilter(),
    'tts.runtime': newFilter(), 'tts.model': newFilter(),
    'ocr.runtime': newFilter(), 'ocr.model': newFilter(),
  });
  let feature = $state<Feature>(catalogView.feature);
  const cleanupSelected = $derived(catalogView.kind === "model" && catalogView.panel === "cleanup");
  $effect(()=>{try {localStorage.setItem("simple-stt-model-page",catalogView.kind);}catch { /* Storage can be disabled. */ }});
  $effect(()=>{ feature=catalogView.feature; });
  $effect(() => { catalogView.feature = feature; });
  $effect(() => { markDirty(); });
  const engines = $derived([...new Map(modules.runtimes
    .filter(runtime => runtime.feature === feature && runtime.installed)
    .map(runtime => [runtime.engine, {
      id: runtime.engine,
      label: runtime.label.replace(/\s*[·–—-]\s*(CPU|CUDA|GPU|Vulkan).*$/i, '').replace(/\s+\(?(CPU|CUDA|GPU|Vulkan).*$/i, ''),
    }])).values()]);
  const jobs = $derived(modules.assets?.jobs ?? (modules.assets?.progress ? [modules.assets.progress] : []));
  const installedRuntimes = $derived(modules.runtimes.filter(runtime => runtime.feature === feature && runtime.installed));
  function catalogRows(kind: Kind, filter: CatalogFilter, selectedEngine: string) {
    return [
    ...modules.runtimes.map(runtime => ({ ...runtime, kind: 'runtime' as const, languages: [] as string[], search: moduleSearchText(runtime, modules.models, modules.runtimes, modules.assets?.assets) })),
    ...modules.models.map(model => ({ ...model, kind: 'model' as const, search: moduleSearchText(model, modules.models, modules.runtimes, modules.assets?.assets) })),
  ].filter(row => row.feature === feature && row.kind === kind
    && (selectedEngine === 'all' || row.engine === selectedEngine)
    && (!filter.installed || row.installed) && matchesCatalogSearch(filter.query, row, row.search))
    .sort((a, b) => Number(!!b.recommended) - Number(!!a.recommended) || a.label.localeCompare(b.label));
  }

  async function refresh() {
    try {
      if (ui.state?.service_online) await action('refresh_models');
      await refreshModules();
    } catch (error) { notice(t('assets.failed'), 'error', String(error)); }
  }
  async function assetAction(name: string, id: string) {
    try {
      await api('/api/asset-action', { action: name, id });
      await refreshModules();
    } catch (error) { notice(t('assets.failed'), 'error', String(error)); }
  }
  async function cancelDownload(id: string, asset: string | null) {
    try {
      if (asset) await assetAction('cancel', asset);
      else await action('cancel_model_download', id);
    } catch (error) { notice(t('assets.failed'), 'error', String(error)); }
  }
  function percent(downloaded: number, total: number) {
    return Math.min(100, Math.floor(downloaded / total * 100));
  }
  function megabytes(bytes: number) { return (bytes / 1024 / 1024).toFixed(1); }
  function languageSummary(languages: string[]) {
    return languages.slice(0, 3).map(languageName).join(', ');
  }
  function size(id: string, asset: string | null) {
    return asset ? modules.assets?.assets.find(entry => entry.id === asset)?.bytes
      : ((ui.state?.models.find(model => model.file === id)?.size_mb || 0) * 1024 * 1024);
  }
  onMount(() => {
    void refreshModules().catch(error => notice(t('assets.failed'), 'error', String(error)));
    void loadReading().catch(error => notice(t('reading.failed'), 'error', String(error)));
  });
</script>

<div class="feature-catalogs" role="group" aria-label={t('settings.downloads')}>
  {#each ['model','runtime'] as kind}<button type="button" data-download-kind={kind} class:chosen={catalogView.kind===kind} aria-pressed={catalogView.kind===kind} onclick={()=>catalogView.kind=kind as Kind}><Icon name={kind==='model'?'models':'advanced'}/><span>{t(kind==='model'?'ui.models.f3798f':'module.runtimes')}</span></button>{/each}
</div>
<div class="feature-catalogs" role="group" aria-label={t('module.feature_filter')}>
  {#each features as option}
    <button type="button" data-catalog-feature={option.id} class:chosen={!cleanupSelected && feature === option.id}
      aria-pressed={!cleanupSelected && feature === option.id} onclick={() => {feature = option.id;catalogView.panel="models";}}>
      <Icon name={option.icon} /><span>{t(option.label)}</span>
    </button>
  {/each}
  {#if catalogView.kind === "model"}<button type="button" data-catalog-feature="cleanup" class:chosen={cleanupSelected} aria-pressed={cleanupSelected} onclick={()=>catalogView.panel="cleanup"}><Icon name="cleanup"/><span>{t("ui.text_cleanup.52f093")}</span></button>{/if}
</div>
{#if cleanupSelected}
<Cleanup section="provider"/>
{:else}
{#if catalogView.kind === "model"}<div class="selection-section" data-catalog-selection={feature}>
  {#if feature === 'stt'}<SttModelSetup/>
  {:else}<ReaderModelSetup {feature}/>{/if}
</div>{/if}
{#each kinds.filter(item=>item.id===catalogView.kind) as section (section.id)}
{@const kind = section.id}
{@const filter = filters[`${feature}.${kind}`]}
{@const selectedEngine = engines.some(option => option.id === filter.engine) ? filter.engine : 'all'}
{@const rows = catalogRows(kind, filter, selectedEngine)}
{@const searchLabel = t(kind === 'runtime' ? 'module.search_runtimes' : 'module.search_models')}
<section class="catalog" id="{kind}-section" aria-labelledby="{kind}-heading" data-catalog="{feature}.{kind}">
  <h2 id="{kind}-heading">{t(section.label)}</h2>
  <div class="catalog-toolbar" data-setting-path="{kind}-search">
    <div class="search-input"><Icon name="search" /><input id="{kind}-search" type="search" dir="auto" aria-label={searchLabel}
      placeholder={searchLabel} bind:value={filter.query} oninput={() => filter.limit = 30} /></div>
    <IconButton icon="refresh" label="module.refresh" onclick={refresh} />
  </div>
  <div class="library-filters">
    {#if engines.length > 1}
      <div class="engine-filters" role="group" aria-label={t('module.engine_filter')}>
        <button type="button" class:chosen={selectedEngine === 'all'} aria-pressed={selectedEngine === 'all'} onclick={() => { filter.engine = 'all'; filter.limit = 30; }}>{t('module.all_engines')}</button>
        {#each engines as option (option.id)}<button type="button" class:chosen={selectedEngine === option.id} aria-pressed={selectedEngine === option.id} onclick={() => { filter.engine = option.id; filter.limit = 30; }}>{option.label}</button>{/each}
      </div>
    {/if}
    <label class="installed-filter"><input type="checkbox" bind:checked={filter.installed} />{t('ui.installed.7bb440')}</label>
  </div>
  <div id="{kind}-results" class="model-results">
    {#each rows.slice(0, filter.limit) as row (`${row.kind}-${row.id}`)}
      {@const asset = modules.assets?.assets.find(entry => entry.id === row.asset)}
      {@const job = row.asset ? jobs.find(job => (job.job_id || job.id) === row.asset) : ui.downloads[row.id]}
      {@const phase = job?.phase || (job ? 'downloading' : '')}
      {@const busy = ['queued', 'downloading', 'verifying', 'extracting'].includes(phase)}
      {@const title = row.kind === 'runtime' ? runtimeLabel(row) : row.label}
      <article class="model-result" data-model-id={row.id}>
        <div class="model-copy">
          <strong dir="auto">{title}</strong>
          {#if row.recommended}<span class="recommendation" title={recommendationLabel(row.recommendation)}><Icon name="check" size={14} />{t('ui.recommended.9ef937')}</span>{/if}
          {#if row.kind === 'model' && row.installed && !installedRuntimes.some(runtime => runtime.engine === row.engine)}<span>{t('module.runtime_missing')}</span>{/if}
          {#if row.languages.length || size(row.id, row.asset)}
            <span>{languageSummary(row.languages)}{#if row.languages.length > 3}{' '}<bdi dir="ltr">+{row.languages.length - 3}</bdi><Help text={row.languages.map(languageName).join(', ')} />{/if}{#if row.languages.length && size(row.id, row.asset)}{' · '}{/if}{#if size(row.id, row.asset)}<bdi dir="ltr">{Math.ceil(size(row.id, row.asset)! / 1024 / 1024)} MB</bdi>{/if}</span>
          {/if}
          <small dir="ltr">{row.kind === 'runtime' ? row.label : row.id}</small>
        </div>
        <div class="model-actions">
          {#if busy}<button type="button" aria-label={`${t('assets.cancel')} ${title}`} onclick={() => cancelDownload(row.id, row.asset)}>{t('assets.cancel')}</button>
          {:else if row.installed}<span title={t('ui.installed.7bb440')}>✓</span>
            {#if row.asset}<IconButton icon="trash" label="assets.remove" onclick={() => assetAction('remove', row.asset!)} />
            {:else if row.kind === 'model'}<IconButton icon="trash" label="assets.remove" disabled={!ui.state?.service_online} onclick={() => action('remove_model', row.id).then(refreshModules)} />{/if}
          {:else if row.asset}<IconButton icon="download" label="assets.download" disabled={!asset?.available} onclick={() => assetAction('install', row.asset!)} />
          {:else if row.kind === 'model'}<IconButton icon="download" label="assets.download" disabled={!ui.state?.service_online} onclick={() => download(row.id)} />
          {:else}<small>{t('ui.unavailable.2c9c1f')}</small>{/if}
        </div>
        {#if busy && job}
          <div class="item-download" role="status">
            <div class="download-detail"><span>{t(phase === 'queued' ? 'module.queued' : `assets.${phase}`)}</span>
              {#if job.total > 0}<bdi dir="ltr">{megabytes(job.downloaded)} / {megabytes(job.total)} MB · {percent(job.downloaded, job.total)}%</bdi>{/if}
            </div>
            <progress max={job.total || 1} value={job.total > 0 ? job.downloaded : undefined} aria-label={`${t('assets.downloading')} ${title}`}></progress>
          </div>
        {:else if phase === 'failed' && job && 'error' in job && job.error}
          <details class="download-error"><summary>{t('assets.failed')} · {t('ui.details.dc3dec')}</summary><pre dir="auto">{job.error}</pre></details>
        {/if}
      </article>
    {/each}
    {#if !rows.length}<p class="empty" role="status">{t('module.no_results')}</p>{/if}
    {#if rows.length > filter.limit}<button type="button" onclick={() => filter.limit += 30}>{t('module.more')}</button>{/if}
  </div>
</section>
{/each}

{#if catalogView.kind === "model"}<ModelFiles {feature}/>{/if}
{/if}

<style>
  .feature-catalogs { display: flex; border-block-end: 1px solid var(--line); gap: 8px; margin-block-end: 20px; }
  .feature-catalogs button { display: flex; align-items: center; justify-content: center; gap: 8px; flex: 1; min-width: 0; padding: 12px 8px; border: 0; border-block-end: 2px solid transparent; border-radius: 0; background: transparent; }
  .feature-catalogs button.chosen { color: var(--accent); border-block-end-color: var(--accent); }
  .selection-section { margin-block-end: 28px; }
  .catalog + .catalog { border-block-start: 1px solid var(--line); margin-block-start: 32px; padding-block-start: 24px; }
  .catalog h2 { margin-block: 0 16px; font-size: 17px; }
  .engine-filters button.chosen { background: var(--accent-soft); color: var(--accent); border-color: var(--accent); }
  .library-filters { display: flex; gap: 12px; flex-wrap: wrap; align-items: center; margin-block: 0 8px; }
  .engine-filters { display: flex; gap: 6px; flex-wrap: wrap; }
  .engine-filters button { border-radius: 999px; padding: 5px 12px; }
  .installed-filter { display: flex; align-items: center; gap: 8px; }
  .model-copy .recommendation { display: flex; align-items: center; gap: 4px; color: var(--accent); }
  .model-copy small { display: block; }
  .model-result { display: grid; grid-template-columns: minmax(0, 1fr) auto; }
  .item-download, .download-error { grid-column: 1 / -1; min-width: 0; }
  .download-detail { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; flex-wrap: wrap; color: var(--muted); font-size: 12px; }
  .download-detail bdi { font-variant-numeric: tabular-nums; }
  progress { display: block; position: static; inset: auto; width: 100%; margin-block: 8px 0; height: 5px; }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; }
  @media (max-width: 600px) {
    .feature-catalogs { gap: 4px; }
    .feature-catalogs button { display: grid; grid-template-columns: minmax(0, 1fr); grid-template-rows: minmax(3em, auto) auto; gap: 4px; padding-inline: 4px; line-height: 1.5; }
    .feature-catalogs button :global(svg) { display: none; }
  }
</style>
