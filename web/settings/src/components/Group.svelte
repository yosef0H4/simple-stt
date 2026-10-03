<script lang="ts">
  import type { Snippet } from "svelte";
  import IconButton from "./IconButton.svelte";
  import Icon from "./Icon.svelte";
  import { resetPaths } from "../lib/state.svelte";
  import { t } from "../lib/i18n";
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

<section class="setting-group" aria-label={t(title)}>
  <header class="group-head">
    <h2><Icon name={icon} size={18} />{t(title)}</h2>
    <div class="group-actions">
      {#if actions}{@render actions()}{/if}{#if paths.length}<IconButton
          icon="reset"
          label={t("ui.reset_group_to_defaults.304141", { group: t(title) })}
          title={t("ui.reset_group.073505")}
          onclick={() => resetPaths(paths)}
        />{/if}
    </div>
  </header>
  {@render children()}
</section>
