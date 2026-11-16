<script lang="ts">
  import General from "./General.svelte";
  import Output from "./Output.svelte";
  import { languageName, t } from "../lib/i18n";
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
    { value: "", label: t("ui.system_default.9d8d38") },
    ...(ui.state?.microphones || []).map((m) => ({
      value: m.id,
      label: m.name,
      meta: m.id,
    })),
  ]);
</script>

<Group
  title="ui.microphone.242805"
  icon="audio"
  paths={["audio.preferred_device_id", "audio.gain", "audio.stop_grace_ms"]}
>
  <Combo
    label="ui.microphone.242805"
    path="audio.preferred_device_id"
    value={ui.config?.audio.preferred_device_id || ""}
    items={microphones}
    unavailable
    onselect={(v) => set("audio.preferred_device_id", v)}
  /><Field spec={fields.gain} /><Field spec={fields.stopGrace} />
</Group>
<General section="dictation" />
<Output />
