<script lang="ts">
  import { onDestroy } from "svelte";
  import { api } from "../lib/api";
  import { get, set, run, ui } from "../lib/state.svelte";
  import { hotkeys } from "../lib/settings";
  import IconButton from "./IconButton.svelte";
  import Icon from "./Icon.svelte";
  import { t } from "../lib/i18n";
  let { path, label }: { path: string; label: string } = $props();
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
          {},
          controller?.signal,
        );
        if (
          hotkeys.some(
            ([p]) =>
              p !== path &&
              get(p) === result.hotkey &&
              result.hotkey.toLowerCase() !== "none",
          )
        )
          throw Error(t("ui.that_shortcut_is_already_assigned.fa1844"));
        set(path, result.hotkey);
      } finally {
        dialog.close();
      }
    });
  }
</script>

<div class="field" data-setting-path={path}>
  <label for={path}>{t(label)}</label>
  <div class="control">
    <input
      dir="ltr"
      id={path}
      value={String(get(path) || "None")}
      readonly={ui.state?.platform === "windows"}
      oninput={(event) => set(path, event.currentTarget.value)}
    />{#if ui.state?.platform === "windows"}<IconButton
        icon="keyboard"
        label={t("ui.record_shortcut_label.dc749f", { label: t(label) })}
        onclick={record}
      />{/if}<IconButton
      icon="close"
      label={t("ui.disable_shortcut_label.918007", { label: t(label) })}
      disabled={get(path) === "None"}
      onclick={() => set(path, "None")}
    />
  </div>
</div>
<dialog bind:this={dialog} oncancel={() => controller?.abort()}>
  <Icon name="keyboard" size={28} />
  <h2>{t("ui.press_your_shortcut.64a18e")}</h2>
  <p>{t("ui.escape_to_cancel.a136d0")}</p>
</dialog>
