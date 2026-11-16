<script lang="ts">
  import type { FieldSpec } from "../lib/types";
  import { get, set } from "../lib/state.svelte";
  import Range from "./Range.svelte";
  import Combo from "./Combo.svelte";
  import Help from "./Help.svelte";
  import { t } from "../lib/i18n";
  let { spec, extra }: { spec: FieldSpec; extra?: import("svelte").Snippet } =
    $props();
  const value = $derived(get(spec.path));
  function change(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    let v: unknown = input.value;
    if (spec.type === "checkbox") v = input.checked;
    else if (spec.type === "number" || spec.type === "range") {
      v = input.valueAsNumber;
      if (!Number.isFinite(v)) return;
    }
    set(spec.path, v);
    if (spec.path === "cleanup.enabled" && !v)
      set("cleanup.screenshot.enabled", false);
  }
</script>

{#if spec.type === "select"}
<Combo label={spec.label} path={spec.path} value={String(value??"")} items={(spec.options||[]).map(([value,label])=>({value,label}))} help={spec.help||""} disabled={spec.disabled} onselect={value=>set(spec.path,value)}>
 {#snippet extra()}{#if extra}{@render extra()}{/if}{/snippet}
</Combo>
{:else if spec.type === "range"}
<Range label={spec.label} path={spec.path} value={Number(value)} min={spec.min??0} max={spec.max??100} step={spec.step||1} unit={spec.suffix?t(spec.suffix):""} help={spec.help||""} disabled={spec.disabled} onchange={value=>set(spec.path,value)}/>
{:else}
<div
  class="field"
  class:field-disabled={spec.disabled}
  data-setting-path={spec.path}
>
  <div class="field-copy">
    <label for={spec.path}>{t(spec.label)}</label>{#if spec.help}<Help
        text={t(spec.help)}
      />{/if}
  </div>
  <div class="control">
    {#if spec.type === "checkbox"}<input
        id={spec.path}
        type="checkbox"
        role="switch"
        checked={Boolean(value)}
        disabled={spec.disabled}
        onchange={change}
      />
    {:else if spec.type === "textarea"}<textarea
        id={spec.path}
        dir={spec.path === "cleanup.prompt" ? "auto" : "ltr"}
        rows="4"
        value={String(value ?? "")}
        disabled={spec.disabled}
        oninput={change}></textarea>
    {:else}<input
        id={spec.path}
        dir="ltr"
        type={spec.type === "number" ? "number" : "text"}
        value={String(value ?? "")}
        min={spec.min}
        max={spec.max}
        step={spec.step || "any"}
        disabled={spec.disabled}
        oninput={change}
      />{/if}
    {#if extra}{@render extra()}{/if}
  </div>
</div>

{/if}
