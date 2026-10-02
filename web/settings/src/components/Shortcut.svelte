<script lang="ts">
  import { onDestroy } from "svelte";
  import { api } from "../lib/api";
  import { get, set, run, ui } from "../lib/state.svelte";
  import { hotkeys } from "../lib/settings";
  import IconButton from "./IconButton.svelte";
  import Icon from "./Icon.svelte";
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
          throw Error("That shortcut is already assigned.");
        set(path, result.hotkey);
      } finally {
        dialog.close();
      }
    });
  }
</script>

<div class="field" data-setting-path={path}>
  <label for={path}>{label}</label>
  <div class="control">
    <input
      id={path}
      value={String(get(path) || "None")}
      readonly={ui.state?.platform === "windows"}
      oninput={(event) => set(path, event.currentTarget.value)}
    />{#if ui.state?.platform === "windows"}<IconButton
        icon="keyboard"
        label={`Record ${label} shortcut`}
        onclick={record}
      />{/if}<IconButton
      icon="close"
      label={`Disable ${label} shortcut`}
      disabled={get(path) === "None"}
      onclick={() => set(path, "None")}
    />
  </div>
</div>
<dialog bind:this={dialog} oncancel={() => controller?.abort()}>
  <Icon name="keyboard" size={28} />
  <h2>Press your shortcut</h2>
  <p>Escape to cancel</p>
</dialog>
