<script lang="ts">
  import {
    ui,
    set,
    refreshLanguages,
    initializeLanguages,
  } from "../lib/state.svelte";
  import { fields } from "../lib/settings";
  import Group from "../components/Group.svelte";
  import Field from "../components/Field.svelte";
  import Combo from "../components/Combo.svelte";
  import ModelAssignment from "../components/ModelAssignment.svelte";
  import IconButton from "../components/IconButton.svelte";
  const discovery = $derived(
    ui.state?.keyboard_languages || {
      available: false,
      message: "",
      languages: [],
    },
  );
  const removed = $derived(
    Object.keys(ui.config?.speech.language_models || {}).filter(
      (id) => !discovery.languages.some((l) => l.id === id),
    ),
  );
  const microphones = $derived([
    { value: "", label: "System default" },
    ...(ui.state?.microphones || []).map((m) => ({
      value: m.id,
      label: m.name,
      meta: m.id,
    })),
  ]);
</script>

<Group
  title="Microphone"
  icon="audio"
  paths={["audio.preferred_device_id", "audio.gain"]}
>
  <Combo
    label="Microphone"
    path="audio.preferred_device_id"
    value={ui.config?.audio.preferred_device_id || ""}
    items={microphones}
    unavailable
    onselect={(v) => set("audio.preferred_device_id", v)}
  /><Field spec={fields.gain} />
</Group>
<Group
  title="Recognition"
  icon="recognition"
  paths={[
    "speech.inference_device",
    "speech.selection_mode",
    "speech.single_model_filename",
    "speech.language_models",
  ]}
>
  <Field spec={fields.device} />
  <div class="field" data-setting-path="speech.selection_mode">
    <label for="selection-mode">Mode</label>
    <div class="control">
      <select
        id="selection-mode"
        value={ui.config?.speech.selection_mode}
        onchange={(event) => {
          set("speech.selection_mode", event.currentTarget.value);
          initializeLanguages();
        }}
        ><option value="single_model">Use one model</option><option
          value="follow_keyboard"
          disabled={!discovery.available &&
            ui.config?.speech.selection_mode !== "follow_keyboard"}
          >Follow keyboard</option
        ></select
      >{#if ui.config?.speech.selection_mode === "follow_keyboard"}<IconButton
          icon="refresh"
          label="Refresh keyboard languages"
          onclick={refreshLanguages}
        />{/if}
    </div>
  </div>
  {#if !discovery.available}<p
      role="status"
      class="inline-status"
      title={discovery.message}
    >
      Keyboard detection unavailable
    </p>{/if}
  {#if ui.config?.speech.selection_mode === "single_model"}<ModelAssignment
      path="speech.single_model_filename"
      label="Model"
    />
  {:else}{#each discovery.languages as language (language.id)}<ModelAssignment
        path={`speech.language_models.${language.id}`}
        label={language.name}
        {language}
      />{/each}
    {#if removed.length}<details class="removed-keyboard-languages">
        <summary title="Not currently on your keyboard"
          >Other languages ({removed.length})</summary
        >{#each removed as id}<ModelAssignment
            path={`speech.language_models.${id}`}
            label={id}
            language={{ id, name: id }}
          />{/each}
      </details>{/if}{/if}
</Group>
