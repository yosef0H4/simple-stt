<script lang="ts">
  import { onDestroy } from "svelte";
  import type { Language } from "../lib/types";
  import { ui, set, get, action } from "../lib/state.svelte";
  import { modelItems } from "../lib/models";
  import Combo from "./Combo.svelte";
  import IconButton from "./IconButton.svelte";
  import { languageName, t } from "../lib/i18n";
  const controller = new AbortController();
  onDestroy(() => controller.abort());
  let {
    path,
    label,
    language,
  }: { path: string; label: string; language?: Language } = $props();
  const value = $derived(String(get(path) || ""));
  const installed = $derived(
    ui.state?.models.some((m) => m.installed && m.file === value),
  );
</script>

<Combo
  {path}
  label={language ? languageName(language.id) : label}
  ariaLabel={language ? languageName(language.id) : t("ui.speech_model.3c5f0b")}
  {value}
  items={modelItems(ui.state?.models || [], language)}
  unavailable={Boolean(value && !installed)}
  onselect={(value) => set(path, value || null)}
>
  {#snippet extra()}<IconButton
      icon="test"
      label={t(path.startsWith("speech.") ? "ui.test_speech_model.05240f" : "ui.test_model.cfb11e")}
      title={t(ui.dirty ? "Save changes before testing" : "Test model")}
      modelTest
      disabled={!value || !installed || ui.dirty || !ui.state?.service_online}
      onclick={() =>
        action(
          "test_model",
          value,
          language?.id === "ar" ? "arabic" : "english",
          controller.signal,
        )}
    />{/snippet}
</Combo>
