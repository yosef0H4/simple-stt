<script lang="ts">
  import { ui, platformAction, set } from "../lib/state.svelte";
  import { t } from "../lib/i18n";
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
  title="ui.shortcuts.90c935"
  icon="keyboard"
  paths={[
    "general.enabled",
    "general.recording_mode",
    "general.linux_hotkey_backend",
    ...hotkeys.map(([p]) => p),
  ]}
>
  {#snippet actions()}{#if ui.state?.platform === "linux"}<IconButton
        icon="refresh"
        label="ui.refresh_or_register_system_shortcuts.5cdfff"
        onclick={() => platformAction("sync_shortcuts")}
      /><IconButton
        icon="keyboard"
        label="ui.configure_system_shortcuts.4c779e"
        onclick={() => platformAction("configure_shortcuts")}
      />{/if}{/snippet}
  <Field spec={fields.enabled} />
  {#if ui.state?.platform === "linux"}
    <Field spec={fields.shortcutSystem} />
    {#if backend === "x11"}{#each hotkeys as [path, label]}<Shortcut
          {path}
          {label}
        />{/each}
    {:else if backend === "portal"}{#each ["record", "cancel", "delivery", "cleanup", "retry"] as id, i}<div
          class="field"
          data-setting-path={`linux.shortcut.${id}`}
        >
          <span>{t(hotkeys[i][1])}</span><output
          >{ui.state.shortcut_state?.[id] || t("ui.not_assigned.b781e6")}</output
          >
        </div>{/each}
    {:else}{#each ["toggle", "cancel", "cycle-delivery", "toggle-cleanup", "retry-delivery"] as command, i}<div
          class="field"
        >
          <span>{t(hotkeys[i][1])}</span><code>simple-stt-linux {command}</code>
        </div>{/each}{/if}
    <details class="guide">
      <summary>{t("ui.setup_guide.6b2f07")}</summary>
      <p>
        {tools.desktop} · {tools.session} · {ui.state.linux_hotkeys?.status ||
          t("ui.not_started.db2c45")}
      </p>
      {#if ui.state.linux_hotkeys?.error}<p>
          {ui.state.linux_hotkeys.error}
        </p>{/if}
      <p>
        {t("ui.desktop_portal_shortcuts_are_assigned.48b1ea")}
      </p>
      <p>
        {t("ui.linux_support_is_experimental_kde.199d9c")}
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
  title="ui.system.bc0792"
  icon="system"
  paths={[
    "general.capslock_behavior",
    "general.start_at_login",
    "general.ui_theme",
    "general.ui_language",
  ]}
>
  {#if ui.state?.platform !== "linux"}<Field spec={fields.caps} />{/if}<div class="field" data-setting-path="general.ui_language">
    <label for="ui-language">{t("ui.language.89b86a")}</label>
    <div class="control"><select id="ui-language" value={ui.config?.general.ui_language || "auto"} onchange={(event) => set("general.ui_language", event.currentTarget.value)}>
      <option value="auto">{t("ui.system.bc0792")}</option>
      <option value="en">{t("ui.english.649df0")}</option>
      <option value="ar">{t("ui.label.997063")}</option>
    </select></div>
  </div><Field
    spec={fields.startup}
  /><Field spec={fields.theme} />
</Group>
