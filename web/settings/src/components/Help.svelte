<script lang="ts">
  import Icon from "./Icon.svelte";
  let { text }: { text: string } = $props();
</script>

<span class="help"
  ><button type="button" aria-label={text} title={text}
    ><Icon name="help" size={15} /></button
  ><span role="tooltip">{text}</span></span
>
