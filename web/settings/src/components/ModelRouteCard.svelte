<script lang="ts">
  import type { Snippet } from "svelte";

  let { id, title, nested = false, actions, children }: {
    id: string;
    title: string;
    nested?: boolean;
    actions?: Snippet;
    children: Snippet;
  } = $props();
</script>

<section class="route-card" class:nested data-route-card={id} aria-labelledby={`${id}-heading`}>
  <header>
    {#if nested}
      <h4 id={`${id}-heading`}>{title}</h4>
    {:else}
      <h3 id={`${id}-heading`}>{title}</h3>
    {/if}
    {@render actions?.()}
  </header>
  {@render children()}
</section>

<style>
  .route-card {
    min-inline-size: 0;
    padding: 12px 16px 8px;
    border: 1px solid var(--line);
    border-radius: 12px;
    background: var(--raised);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding-block-end: 7px;
    border-block-end: 1px solid var(--line);
  }
  h3, h4 { margin: 0; font-size: 14px; font-weight: 650; }
  h4 { font-size: 13px; }
  .nested {
    margin-block: 4px 8px;
    background: var(--paper);
  }
  .route-card :global(.field) { padding-block: 7px; }
  .route-card :global(.inline-status) { margin-block: 5px; }
  @media (max-width: 700px) {
    .route-card { padding-inline: 10px; }
  }
</style>
