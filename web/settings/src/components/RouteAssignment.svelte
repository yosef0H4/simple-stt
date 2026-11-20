<script lang="ts">
  import { t, languageName } from "../lib/i18n";
  import { markDirty } from "../lib/state.svelte";
  import type { Binding, Feature, RemoteRoute, Connection } from "../lib/modules.svelte";
  import EngineAssignment from "./EngineAssignment.svelte";
  import ConnectionEditor from "./ConnectionEditor.svelte";
  import ModelRouteCard from "./ModelRouteCard.svelte";
  import IconButton from "./IconButton.svelte";

  let { feature, language, path, binding, defaultRemote, defaultFallback, onremove, onchange }: {
    feature: Feature;
    language: string;
    path: string;
    binding: Binding;
    defaultRemote: boolean;
    defaultFallback: boolean;
    onremove?: () => void;
    onchange: (value: Binding) => void;
  } = $props();

  const title = $derived(language ? languageName(language) : t("settings.model_route_title"));
  const selected = $derived(binding.remote ? binding.remote.enabled ? "remote" : "local" : "default");
  const remote = $derived(binding.remote ?? {
    enabled: false,
    primary: { base_url: "", model: "", voice: "", format: "wav", timeout_ms: 60000, key_slot: `${path}.primary` } satisfies Connection,
    fallback_to_local: false,
  });
  const localFallback = $derived(selected === "remote" ? remote.fallback_to_local :
    selected === "default" && defaultRemote && defaultFallback);

  function update(value: RemoteRoute | null) {
    onchange({ ...binding, remote: value });
    markDirty();
  }
</script>

{#snippet local()}
  <EngineAssignment {feature} {language} {path} {binding} {onchange} />
{/snippet}

<ModelRouteCard id={path} {title}>
  {#snippet actions()}
    {#if onremove}
      <IconButton icon="close" label={`${t("module.remove_language")} ${title}`} onclick={onremove} />
    {/if}
  {/snippet}
  {#if feature !== "ocr"}
    <div class="field">
      <label for={`${path}-route`}>{t("settings.run_model")}</label>
      <div class="control">
        <select id={`${path}-route`} value={selected} onchange={event => {
          const value = event.currentTarget.value;
          update(value === "default" ? null : { ...remote, enabled: value === "remote" });
        }}>
          <option value="default">{t("settings.default_route")} · {t(defaultRemote ? "ui.openai_compatible.b10d74" : "module.local_models")}</option>
          <option value="local">{t("module.local_models")}</option>
          <option value="remote">{t("ui.openai_compatible.b10d74")}</option>
        </select>
      </div>
    </div>
    {#if selected === "remote"}
      <ConnectionEditor {feature} path={`${path}.primary`} connection={remote.primary} onchange={primary => update({ ...remote, primary })} />
      <div class="field">
        <label for={`${path}-local-fallback`}>{t("reading.fallback_to_local")}</label>
        <div class="control"><input id={`${path}-local-fallback`} type="checkbox" role="switch" checked={remote.fallback_to_local} onchange={event => update({ ...remote, fallback_to_local: event.currentTarget.checked })} /></div>
      </div>
    {/if}
  {/if}
  {#if localFallback}
    <ModelRouteCard id={`${path}.local-fallback`} title={t("settings.local_fallback")} nested>
      {@render local()}
    </ModelRouteCard>
  {:else if feature === "ocr" || selected === "local" || (selected === "default" && !defaultRemote)}
    {@render local()}
  {/if}
</ModelRouteCard>
