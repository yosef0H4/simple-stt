<script lang="ts">
  import type { FieldSpec } from "../lib/types";
  import { get, set } from "../lib/state.svelte";
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
    {:else if spec.type === "select"}<select
        id={spec.path}
        value={String(value ?? "")}
        disabled={spec.disabled}
        onchange={change}
        >{#each spec.options || [] as [v, label]}<option value={v}
            >{t(label)}</option
          >{/each}</select
      >
    {:else if spec.type === "textarea"}<textarea
        id={spec.path}
        dir={spec.path === "cleanup.prompt" ? "auto" : "ltr"}
        rows="4"
        value={String(value ?? "")}
        disabled={spec.disabled}
        oninput={change}></textarea>
    {:else if spec.type === "range"}<input
        id={spec.path}
        type="range"
        value={Number(value)}
        min={spec.min}
        max={spec.max}
        step={spec.step || 1}
        disabled={spec.disabled}
        oninput={change}
      /><output class="range-value">{String(value)}{spec.suffix ? t(spec.suffix) : ""}</output>
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
