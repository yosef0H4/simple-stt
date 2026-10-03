<script lang="ts">
  import Icon from "./Icon.svelte";
  import { run } from "../lib/state.svelte";
  import { t } from "../lib/i18n";
  let {
    icon,
    label,
    title = label,
    disabled = false,
    onclick,
    kind = "",
    modelTest = false,
  }: {
    icon: string;
    label: string;
    title?: string;
    disabled?: boolean;
    onclick?: () => unknown | Promise<unknown>;
    kind?: string;
    modelTest?: boolean;
  } = $props();
  let busy = $state(false);
  async function click() {
    busy = true;
    await run(async () => onclick?.());
    busy = false;
  }
</script>

<button
  type="button"
  class="icon-button {kind}"
  aria-label={t(label)}
  title={t(title)}
  disabled={disabled || busy}
  aria-busy={busy}
  data-model-test={modelTest ? "true" : undefined}
  onclick={click}><Icon name={busy ? "refresh" : icon} /></button
>
