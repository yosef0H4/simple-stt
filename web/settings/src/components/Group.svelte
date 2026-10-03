<script lang="ts">
  import type { Snippet } from "svelte";
  import IconButton from "./IconButton.svelte";
  import Icon from "./Icon.svelte";
  import { resetPaths } from "../lib/state.svelte";
  let {
    title,
    icon,
    paths = [],
    children,
    actions,
  }: {
    title: string;
    icon: string;
    paths?: string[];
    children: Snippet;
    actions?: Snippet;
  } = $props();
</script>

<section class="setting-group" aria-label={title}>
  <header class="group-head">
    <h2><Icon name={icon} size={18} />{title}</h2>
    <div class="group-actions">
      {#if actions}{@render actions()}{/if}{#if paths.length}<IconButton
          icon="reset"
          label={`Reset ${title} to defaults`}
          title="Reset group"
          onclick={() => resetPaths(paths)}
        />{/if}
    </div>
  </header>
  {@render children()}
</section>
