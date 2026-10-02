<script lang="ts">
  import { ui, platformAction } from "../lib/state.svelte";
  import { fields, hotkeys } from "../lib/settings";
  import Group from "../components/Group.svelte";
  import Field from "../components/Field.svelte";
  import Shortcut from "../components/Shortcut.svelte";
  import IconButton from "../components/IconButton.svelte";
  const tools = $derived(ui.state?.linux_automation || {});
  const selected = $derived(ui.config?.general.linux_hotkey_backend || "auto");
  const backend = $derived(
    ui.state?.linux_hotkeys?.requested === selected &&
      ui.state.linux_hotkeys.active
      ? ui.state.linux_hotkeys.active
      : selected === "auto"
        ? tools.session === "X11"
          ? "x11"
          : "portal"
        : selected,
  );
</script>

<Group
  title="Shortcuts"
  paths={[
    "general.enabled",
    "general.recording_mode",
    "general.linux_hotkey_backend",
    ...hotkeys.map(([p]) => p),
  ]}
>
  {#snippet actions()}{#if ui.state?.platform === "linux"}<IconButton
        icon="refresh"
        label="Refresh or register system shortcuts"
        onclick={() => platformAction("sync_shortcuts")}
      /><IconButton
        icon="keyboard"
        label="Configure system shortcuts"
        onclick={() => platformAction("configure_shortcuts")}
      />{/if}{/snippet}
  <Field spec={fields.enabled} />
  {#if ui.state?.platform === "linux"}
    <Field spec={fields.shortcutSystem} />
    {#if backend === "x11"}{#each hotkeys as [path, label]}<Shortcut
          {path}
          {label}
        />{/each}
    {:else if backend === "portal"}{#each ["record", "cancel", "delivery", "cleanup"] as id, i}<div
          class="field"
          data-setting-path={`linux.shortcut.${id}`}
        >
          <span>{hotkeys[i][1]}</span><output
            >{ui.state.shortcut_state?.[id] || "Not assigned"}</output
          >
        </div>{/each}
    {:else}{#each ["toggle", "cancel", "cycle-delivery", "toggle-cleanup"] as command, i}<div
          class="field"
        >
          <span>{hotkeys[i][1]}</span><code>simple-stt-linux {command}</code>
        </div>{/each}{/if}
    <details class="guide">
      <summary>Setup guide</summary>
      <p>
        {tools.desktop} · {tools.session} · {ui.state.linux_hotkeys?.status ||
          "Not started"}
      </p>
      {#if ui.state.linux_hotkeys?.error}<p>
          {ui.state.linux_hotkeys.error}
        </p>{/if}
      <p>
        Desktop portal shortcuts are assigned in your desktop’s system dialog.
        X11 chords are saved here. Desktop commands are bound in your
        compositor.
      </p>
      <p>
        Linux support is experimental; KDE Plasma Wayland is tested on real
        hardware.
      </p>
      <code
        >{tools.start_command ||
          "systemctl --user start simple-stt-linux.service"}</code
      ><code>{tools.stop_command || "simple-stt-linux shutdown"}</code>
    </details>
  {:else}<Field
      spec={fields.recordingMode}
    />{#each hotkeys as [path, label]}<Shortcut {path} {label} />{/each}{/if}
</Group>
<Group
  title="System"
  paths={[
    "general.capslock_behavior",
    "general.start_at_login",
    "general.ui_theme",
  ]}
>
  {#if ui.state?.platform !== "linux"}<Field spec={fields.caps} />{/if}<Field
    spec={fields.startup}
  /><Field spec={fields.theme} />
</Group>
