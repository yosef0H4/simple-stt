<script lang="ts">
  import type { ComboItem } from "../lib/types";
  import Icon from "./Icon.svelte";
  import { tick, type Snippet } from "svelte";
  import Help from "./Help.svelte";
  let {
    label,
    path,
    value,
    items,
    onselect,
    custom = false,
    unavailable = false,
    help = "",
    extra,
  }: {
    label: string;
    path: string;
    value: string;
    items: ComboItem[];
    onselect: (value: string) => void;
    custom?: boolean;
    unavailable?: boolean;
    help?: string;
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
      (unavailable ? `Unavailable · ${value}` : value),
  );
  const filtered = $derived(
    items.filter((i) =>
      `${i.label} ${i.meta || ""}`.toLowerCase().includes(query.toLowerCase()),
    ),
  );
  function expand() {
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
    <label for={`combo-${path}`}>{label}</label>{#if help}<Help
        text={help}
      />{/if}
  </div>
  <div class="control assignment-control">
    <div class="combo model-combo">
      <div class="combo-shell">
        <input
          bind:this={input}
          id={`combo-${path}`}
          class="combo-input"
          type="text"
          role="combobox"
          aria-label={label === "Model"
            ? "Speech model"
            : label.endsWith("model")
              ? label
              : `${label} model`}
          aria-autocomplete="list"
          aria-expanded={open}
          aria-controls={`list-${path}`}
          aria-activedescendant={open && active >= 0
            ? `option-${path}-${active}`
            : undefined}
          autocomplete="off"
          value={open ? query : selected}
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
    aria-label={`${label} choices`}
    style:left={`${position.left}px`}
    style:top={`${position.top}px`}
    style:width={`${position.width}px`}
    style:max-height={`${position.height}px`}
  >
    {#each filtered as item, i (item.value)}<button
        type="button"
        id={`option-${path}-${i}`}
        role="option"
        aria-selected={value === item.value}
        class:highlighted={active === i}
        onpointerdown={(event) => event.preventDefault()}
        onclick={() => choose(item)}
        >{item.label}{#if item.value === value}<Icon
            name="check"
            size={15}
          />{/if}</button
      >{:else}<span class="empty">No matches</span>{/each}
  </div>{/if}
