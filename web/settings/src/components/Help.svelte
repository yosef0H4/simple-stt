<script lang="ts">
  import Icon from "./Icon.svelte";
  import { t } from "../lib/i18n";
  let { text }: { text: string } = $props();
</script>

<span class="help"
  ><button type="button" aria-label={t(text)} title={t(text)}
    ><Icon name="help" size={15} /></button
  ></span
>
