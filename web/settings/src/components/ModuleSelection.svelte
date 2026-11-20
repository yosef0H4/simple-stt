<script lang="ts">
  import { onMount, tick, type Snippet } from "svelte";
  import { t, languageName } from "../lib/i18n";
  import { ui, set, refreshLanguages, notice } from "../lib/state.svelte";
  import { reading } from "../lib/reading-state.svelte";
  import { modules, refreshModules, emptyBinding, type Binding, type Feature, type Selection } from "../lib/modules.svelte";
  import Combo from "./Combo.svelte";
  import IconButton from "./IconButton.svelte";
  import ModelRouteCard from "./ModelRouteCard.svelte";
  import RouteAssignment from "./RouteAssignment.svelte";
  import EngineAssignment from "./EngineAssignment.svelte";

  let { feature, provider, defaultRemote, defaultFallback }: {
    feature: Feature;
    provider: Snippet;
    defaultRemote: boolean;
    defaultFallback: boolean;
  } = $props();

  let adding = $state(false);
  const readerSection = $derived(feature === "ocr" ? "ocr" : "speech");
  const mode = $derived(feature === "stt" ? ui.config?.speech.selection_mode : selection()?.mode);
  const single = $derived(mode === "single_model");
  const detected = $derived(mode === "auto_detect" && feature === "tts");
  const configured = $derived(Object.keys(selection()?.languages || {}).sort());
  const languages = $derived(detected ? configured : (ui.state?.keyboard_languages?.languages || []).map(language => language.id));
  const supported = $derived([...new Set(modules.models.filter(model => model.feature === "tts").flatMap(model => model.languages))].sort());
  const addable = $derived(supported.filter(code => !configured.includes(code)).map(code => ({ value: code, label: languageName(code) })));
  const singleBinding = $derived(binding(""));
  const singleSpeech = $derived(single && feature === "tts");

  function selection(): Selection | undefined {
    return reading.config?.[readerSection].selection;
  }

  function binding(language: string): Binding {
    if (feature === "stt") {
      const speech = ui.config?.speech;
      return {
        remote: language ? speech?.remote_languages?.[language] : undefined,
        runtime: language ? speech?.language_runtimes?.[language] ?? null : speech?.runtime_dir.split(/[\\/]/).pop() || null,
        model: language ? speech?.language_models[language] || null : speech?.single_model_filename || null,
        device: language ? speech?.language_devices?.[language] || "auto" : speech?.inference_device || "auto",
      };
    }
    const choices = selection();
    return (language ? choices?.languages[language] : choices?.single) || emptyBinding();
  }

  function update(value: Binding, language: string) {
    if (feature !== "stt") {
      const choices = selection();
      if (choices) {
        if (language) choices.languages[language] = value;
        else choices.single = value;
      }
      return;
    }
    const speech = ui.config?.speech;
    if (!speech) return;
    if (!language) {
      set("speech.single_model_filename", value.model);
      set("speech.runtime_dir", modules.runtimes.find(runtime => runtime.id === value.runtime)?.directory || "");
      set("speech.inference_device", value.device);
      return;
    }
    const remote = { ...speech.remote_languages };
    if (value.remote) remote[language] = value.remote;
    else delete remote[language];
    set("speech.remote_languages", remote);
    set(`speech.language_models.${language}`, value.model);
    set("speech.language_runtimes", { ...speech.language_runtimes, [language]: value.runtime });
    set("speech.language_devices", { ...speech.language_devices, [language]: value.device });
  }

  async function addLanguage(code: string) {
    const choices = selection();
    if (!choices || !code || choices.languages[code]) return;
    choices.languages[code] = {
      ...emptyBinding(),
      clean_text: choices.single.clean_text,
      currency_words: choices.single.currency_words,
      speed_percent: choices.single.speed_percent,
    };
    adding = false;
    await tick();
    document.getElementById(`tts.${code}-route`)?.focus();
  }

  async function startAdding() {
    adding = true;
    await tick();
    document.getElementById("combo-tts.add-language")?.focus();
  }

  function removeLanguage(code: string) {
    const choices = selection();
    if (choices) delete choices.languages[code];
  }

  function changeMode(value: Selection["mode"]) {
    adding = false;
    if (feature === "stt") set("speech.selection_mode", value);
    else {
      const choices = selection();
      if (choices) choices.mode = value;
    }
  }

  onMount(() => { void refreshModules().catch(error => notice(t("ui.operation_failed.c4e6ed"), "error", String(error))); });
</script>

<div class="field">
  <label for={`${feature}-mode`}>{t("ui.mode.a7b93d")}</label>
  <div class="control">
    <select id={`${feature}-mode`} value={mode} onchange={event => changeMode(event.currentTarget.value as Selection["mode"])}>
      <option value="single_model">{t("ui.use_one_model.bce8d2")}</option>
      {#if feature === "tts"}<option value="auto_detect">{t("module.auto_detect")}</option>{/if}
      <option value="follow_keyboard">{t("ui.follow_keyboard.565bed")}</option>
    </select>
    {#if mode === "follow_keyboard"}
      <IconButton icon="refresh" label="ui.refresh_keyboard_languages.49fbe8" onclick={refreshLanguages} />
    {/if}
  </div>
</div>

<svelte:window onkeydown={event => { if (event.key === "Escape") adding = false; }} />
<svelte:document onpointerdown={event => {
  if (adding && event.target instanceof Element && !event.target.closest("[data-language-picker]")) adding = false;
}} />

{#snippet languageCards()}
  {#each languages as language (language)}
    <RouteAssignment {feature} {language} path={`${feature}.${language}`}
      binding={binding(language)} defaultRemote={feature !== "ocr" && defaultRemote} {defaultFallback}
      onremove={detected ? () => removeLanguage(language) : undefined}
      onchange={value => update(value, language)} />
  {:else}
    <p class="inline-status">{t(detected ? "module.empty_languages" : "ui.keyboard_detection_unavailable.091ebb")}</p>
  {/each}
{/snippet}

{#snippet singleModel()}
  <EngineAssignment {feature} path={`${feature}.single`} binding={singleBinding} onchange={value => update(value, "")} />
{/snippet}

<div class="route-cards">
  {#if singleSpeech}
    <RouteAssignment {feature} language="" path={`${feature}.single`} binding={singleBinding} {defaultRemote} {defaultFallback}
      onchange={value => update(value, "")} />
  {:else if !single && !(feature === "ocr" && defaultRemote)}
    {@render languageCards()}
  {/if}

  {#if detected}
    <div data-language-picker={feature}>
      {#if adding || !languages.length}
        <Combo label="module.add_language" path="tts.add-language" value="" items={addable} onselect={addLanguage}>
          {#snippet extra()}
            {#if languages.length}
              <IconButton icon="close" label="module.cancel_add_language" onclick={() => adding = false} />
            {/if}
          {/snippet}
        </Combo>
      {:else}
        <button type="button" class="quiet" disabled={!addable.length} onclick={startAdding}>{t("module.add_language")}</button>
      {/if}
    </div>
  {/if}

  <ModelRouteCard id={`${feature}.default`} title={t(single && !singleSpeech ? "settings.model_route_title" : "settings.default_route_title")}>
    {@render provider()}
    {#if single && !singleSpeech && !defaultRemote}
      {@render singleModel()}
    {:else if defaultRemote && defaultFallback && ((single && !singleSpeech) || feature === "ocr")}
      <ModelRouteCard id={`${feature}.default.local-fallback`} title={t("settings.local_fallback")} nested>
        {#if single}
          {@render singleModel()}
        {:else}
          <div class="route-cards">{@render languageCards()}</div>
        {/if}
      </ModelRouteCard>
    {/if}
  </ModelRouteCard>
</div>

<style>
  .route-cards { display: grid; gap: 12px; min-inline-size: 0; }
  .route-cards .route-cards { margin-block-start: 8px; }
</style>
