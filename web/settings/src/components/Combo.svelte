<script lang="ts">
  import type { ComboItem } from "../lib/types";
  import { matchesSearch } from "../lib/search";
  import Icon from "./Icon.svelte";
  import { tick, type Snippet } from "svelte";
  import Help from "./Help.svelte";
  import { t } from "../lib/i18n";
  let {
    label,
    ariaLabel,
    path,
    value,
    items,
    onselect,
    custom = false,
    disabled = false,
    unavailable = false,
    help = "",
    searchMatcher,
    extra,
  }: {
    label: string;
    ariaLabel?: string;
    path: string;
    value: string;
    items: (ComboItem & { search?: string })[];
    onselect: (value: string) => void;
    custom?: boolean;
    disabled?: boolean;
    unavailable?: boolean;
    help?: string;
    searchMatcher?: (query: string, item: ComboItem & { search?: string }) => boolean;
    extra?: Snippet;
  } = $props();
  let open = $state(false),
    query = $state(""),
    active = $state(-1),
    input: HTMLInputElement,
    root: HTMLDivElement;
  let position = $state({ left: 0, top: 0, width: 0, height: 260 });
  const selected = $derived(
    items.find((i) => i.value === value)?.label ||
      (unavailable ? `${t("ui.unavailable.2c9c1f")} · ${value}` : value),
  );
  const filtered = $derived(
    items.filter((i) =>
      searchMatcher ? searchMatcher(query, i) : matchesSearch(query, t(i.label), i.label, i.value, i.meta, i.search),
    ),
  );
  function expand() {
    if(disabled)return;
    open = true;
    query = "";
    active = -1;
    const box = input.getBoundingClientRect();
    const below = window.innerHeight - box.bottom - 12;
    position = {
      left: box.left,
      top: below < 130 ? Math.max(8, box.top - 270) : box.bottom + 5,
      width: box.width,
      height: below < 130 ? 250 : Math.min(280, below),
    };
  }
  function choose(item: ComboItem) {
    onselect(item.value);
    open = false;
    query = "";
    input.focus();
  }
  function close() {
    if (custom && query.trim()) onselect(query.trim());
    open = false;
    query = "";
    active = -1;
  }
  async function key(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      open = false;
      query = "";
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (!open) expand();
      active = Math.max(
        0,
        Math.min(
          filtered.length - 1,
          active + (event.key === "ArrowDown" ? 1 : -1),
        ),
      );
      await tick();
      document
        .getElementById(`option-${path}-${active}`)
        ?.scrollIntoView({ block: "nearest" });
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (active >= 0 && filtered[active]) choose(filtered[active]);
      else if (custom) close();
      else if (filtered.length === 1) choose(filtered[0]);
    }
  }
</script>

<svelte:document
  onpointerdown={(event) => {
    if (
      open &&
      !root.contains(event.target as Node) &&
      !(event.target as Element).closest?.(`[data-combo-list="${path}"]`)
    )
      close();
  }}
/>
<svelte:window onresize={() => (open = false)} />
<div class="field" data-setting-path={path} bind:this={root}>
  <div class="field-copy">
    <label for={`combo-${path}`}>{t(label)}</label>{#if help}<Help
        text={t(help)}
      />{/if}
  </div>
  <div class="control assignment-control">
    <div class="combo model-combo">
      <div class="combo-shell">
        <input
          bind:this={input}
          id={`combo-${path}`}
          class="combo-input"
          {disabled}
          type="text"
          dir="auto"
          role="combobox"
          aria-label={t(ariaLabel || label)}
          aria-autocomplete="list"
          aria-expanded={open}
          aria-controls={`list-${path}`}
          aria-activedescendant={open && active >= 0
            ? `option-${path}-${active}`
            : undefined}
          autocomplete="off"
          value={open ? query : t(selected)}
          onfocus={expand}
          onclick={() => {
            if (!open) expand();
          }}
          oninput={(event) => {
            query = event.currentTarget.value;
            open = true;
            active = -1;
          }}
          onkeydown={key}
          onblur={(event) => {
            if (
              !(event.relatedTarget as HTMLElement)?.closest(
                `[data-combo-list="${path}"]`,
              )
            )
              close();
          }}
        /><Icon name="chevron" size={14} />
      </div>
    </div>
    {#if extra}{@render extra()}{/if}
  </div>
</div>
{#if open}<div
    class="combo-list"
    id={`list-${path}`}
    data-combo-list={path}
    role="listbox"
    aria-label={`${t(label)} ${t("ui.choices.153eb4")}`}
    style:left={`${position.left}px`}
    style:top={`${position.top}px`}
    style:width={`${position.width}px`}
    style:max-height={`${position.height}px`}
  >
    {#each filtered as item, i (item.value)}<button
        type="button"
        id={`option-${path}-${i}`}
        role="option"
        aria-label={t(item.label)}
        aria-describedby={item.meta ? `meta-${path}-${i}` : undefined}
        aria-selected={value === item.value}
        class:highlighted={active === i}
        onpointerdown={(event) => event.preventDefault()}
        onclick={() => choose(item)}
        ><span class="option-copy"><span>{t(item.label)}</span>{#if item.meta}<small id={`meta-${path}-${i}`} dir="auto">{item.meta}</small>{/if}</span>{#if item.value === value}<Icon
            name="check"
            size={15}
          />{/if}</button
      >{:else}<span class="empty">{t("ui.no_matches.cd0af6")}</span>{/each}
  </div>{/if}
<style>
  .option-copy { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .option-copy small { color: var(--muted); font-size: 11px; overflow-wrap: anywhere; }
</style>
