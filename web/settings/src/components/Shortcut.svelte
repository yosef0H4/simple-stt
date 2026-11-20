<script lang="ts">
  import { onDestroy } from "svelte";
  import { api } from "../lib/api";
  import { get, set, run, ui } from "../lib/state.svelte";
  import { reading } from "../lib/reading-state.svelte";
  import { hotkeys } from "../lib/settings";
  import IconButton from "./IconButton.svelte";
  import Icon from "./Icon.svelte";
  import { t } from "../lib/i18n";
  let {
    path,
    label,
    inputId = path,
    value,
    onchange,
  }: { path: string; label: string; inputId?: string; value?: string; onchange?: (value: string) => void } = $props();
  const shortcut = $derived(value ?? String(get(path) || "None"));
  function change(value: string) {
    if (onchange) onchange(value);
    else set(path, value);
  }
  let dialog: HTMLDialogElement;
  let controller: AbortController | undefined;
  onDestroy(() => controller?.abort());
  async function record() {
    controller = new AbortController();
    dialog.showModal();
    await run(async () => {
      try {
        const result = await api<{ hotkey: string }>(
          "/api/hotkey-capture",
          { allow_caps_lock: path === "general.show_shortcuts_hotkey" && get("general.show_shortcuts_mode") === "hold" },
          controller?.signal,
        );
        if (
          result.hotkey.toLowerCase() !== "none" && [
            ...hotkeys.map(([p]) => [p, get(p)]),
            ...Object.entries(reading.config?.shortcuts ?? {}).map(([key, value]) => [`reader.shortcuts.${key}`, value]),
          ].some(([p, assigned]) =>
            p !== path && typeof assigned === "string" &&
            assigned.toLowerCase() === result.hotkey.toLowerCase(),
          )
        )
          throw Error(t("ui.that_shortcut_is_already_assigned.fa1844"));
        change(result.hotkey);
      } finally {
        dialog.close();
      }
    });
  }
</script>

<div class="field" data-setting-path={path}>
  <label for={inputId}>{t(label)}</label>
  <div class="control">
    <input
      dir="ltr"
      id={inputId}
      value={shortcut}
      title={shortcut}
      readonly={ui.state?.platform === "windows"}
      oninput={(event) => change(event.currentTarget.value)}
    />{#if ui.state?.platform === "windows"}<IconButton
        icon="keyboard"
        label={t("ui.record_shortcut_label.dc749f", { label: t(label) })}
        onclick={record}
      />{/if}<IconButton
      icon="close"
      label={t("ui.disable_shortcut_label.918007", { label: t(label) })}
      disabled={shortcut === "None"}
      onclick={() => change("None")}
    />
  </div>
</div>
<dialog bind:this={dialog} oncancel={() => controller?.abort()}>
  <Icon name="keyboard" size={28} />
  <h2>{t("ui.press_your_shortcut.64a18e")}</h2>
  <p>{t("ui.escape_to_cancel.a136d0")}</p>
</dialog>
