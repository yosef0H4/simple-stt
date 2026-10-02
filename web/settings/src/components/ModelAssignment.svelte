<script lang="ts">
  import { onDestroy } from "svelte";
  import type { Language } from "../lib/types";
  import { ui, set, get, action } from "../lib/state.svelte";
  import { modelItems } from "../lib/models";
  import Combo from "./Combo.svelte";
  import IconButton from "./IconButton.svelte";
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
  {label}
  {value}
  items={modelItems(ui.state?.models || [], language)}
  unavailable={Boolean(value && !installed)}
  onselect={(value) => set(path, value || null)}
>
  {#snippet extra()}<IconButton
      icon="test"
      label={`Test ${label === "Model" ? "speech" : label} model`}
      title={ui.dirty ? "Save changes before testing" : "Test model"}
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
