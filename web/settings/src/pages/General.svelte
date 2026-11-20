<script lang="ts">
  import { ui, platformAction, run, set, markDirty, resetPaths } from "../lib/state.svelte";
  import { t } from "../lib/i18n";
  import { onMount } from "svelte";
  import { loadReading, reading } from "../lib/reading-state.svelte";
  import type { ReaderConfig } from "../lib/reader";
  onMount(() => { void loadReading().catch(() => {}); });
  import { fields, hotkeys, readerHotkeys } from "../lib/settings";
  import Group from "../components/Group.svelte";
  import Field from "../components/Field.svelte";
  import Shortcut from "../components/Shortcut.svelte";
  import IconButton from "../components/IconButton.svelte";
  import Icon from "../components/Icon.svelte";
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
  const portalShortcuts = $derived(ui.state?.platform === "linux" && backend === "portal");
  const capsOnlyHint = $derived(ui.config?.general.show_shortcuts_hotkey.trim().toLowerCase() === "capslock");
  const shortcutPaths = [
    "general.enabled",
    "general.shortcut_reminders",
    "general.linux_hotkey_backend",
    "general.recording_mode",
    "general.show_shortcuts_mode",
    ...hotkeys.map(([path]) => path),
  ];
  let configuringShortcuts = $state(false);
  async function resetShortcuts() {
    const resetReader = !portalShortcuts;
    await resetPaths(shortcutPaths);
    if (resetReader && reading.config) {
      for (const [, key] of readerHotkeys) reading.config.shortcuts[key] = "None";
      markDirty();
    }
  }
  function setReaderShortcut(key: keyof ReaderConfig["shortcuts"], value: string) {
    if (reading.config) {
      reading.config.shortcuts[key] = value;
      markDirty();
    }
  }
  async function configureShortcuts() {
    configuringShortcuts = true;
    try {
      await platformAction("configure_shortcuts");
    } finally {
      configuringShortcuts = false;
    }
  }
</script>

{#snippet appShortcut(index: number)}
  {@const [path, label] = hotkeys[index]}
  {#if portalShortcuts}
    {@const id = ["record", "cancel", "delivery", "cleanup", "retry", "shortcuts"][index]}
    <div class="field" data-setting-path={`linux.shortcut.${id}`}>
      <span>{t(label)}</span><output>{ui.state?.shortcut_state?.[id] || t("ui.not_assigned.b781e6")}</output>
    </div>
  {:else if ui.state?.platform === "linux" && backend !== "x11"}
    {@const command = ["toggle", "cancel", "cycle-delivery", "toggle-cleanup", "retry-delivery", "show-shortcuts"][index]}
    <div class="field"><span>{t(label)}</span><code>simple-stt-linux {command}</code></div>
  {:else}
    <Shortcut {path} {label} />
  {/if}
{/snippet}

{#snippet readerShortcut([id, key]: (typeof readerHotkeys)[number])}
  {#if portalShortcuts}
    <div class="field" data-setting-path={`reader.shortcuts.${key}`}><span>{t(`reading.${key}`)}</span><output>{ui.state?.shortcut_state?.[id] || t("ui.not_assigned.b781e6")}</output></div>
  {:else if reading.config}
    {#if ui.state?.platform === "windows"}
      <Shortcut path={`reader.shortcuts.${key}`} inputId={`reader-shortcut-${key}`} label={`reading.${key}`}
        value={reading.config.shortcuts[key]} onchange={(value)=>setReaderShortcut(key,value)} />
    {:else}
      <div class="field" data-setting-path={`reader.shortcuts.${key}`}><label for={`reader-shortcut-${key}`}>{t(`reading.${key}`)}</label><div class="control"><input id={`reader-shortcut-${key}`} dir="ltr" value={reading.config.shortcuts[key]} oninput={(e)=>setReaderShortcut(key,e.currentTarget.value)} /></div></div>
    {/if}
  {/if}
{/snippet}

{#snippet cardHeading(id: string, label: string, icon: string)}
  <h3 id={id}><Icon name={icon} size={17} />{t(label)}</h3>
{/snippet}

<Group
  title="ui.shortcuts.90c935"
  icon="keyboard"
  onreset={() => run(resetShortcuts)}
>
  {#snippet actions()}{#if ui.state?.platform === "linux"}<IconButton
        icon="refresh"
        label="ui.refresh_or_register_system_shortcuts.5cdfff"
        onclick={() => platformAction("sync_shortcuts")}
      /><button
        class="shortcut-setup quiet"
        type="button"
        aria-busy={configuringShortcuts}
        disabled={configuringShortcuts}
        onclick={() => run(configureShortcuts)}
      ><Icon name="keyboard" size={15} /><span>{t("ui.set_up_shortcuts.a104f9")}</span></button
      >{/if}{/snippet}
  <Field spec={fields.enabled} />
  <Field spec={fields.shortcutReminders} />
  <button type="button" class="quiet" onclick={() => run(() => platformAction("show_shortcuts"))}>{t("ui.show_shortcuts")}</button>
  {#if ui.state?.platform === "linux"}
    <Field spec={fields.shortcutSystem} />
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
  {/if}
  <div class="shortcut-cards">
    <section class="shortcut-card" aria-labelledby="shortcuts-app-heading">
      {@render cardHeading("shortcuts-app-heading", "settings.app_wide_shortcuts", "keyboard")}
      {@render appShortcut(5)}
      {#if ui.state?.platform === "windows"}
        <Field spec={{ ...fields.showShortcutsMode, disabled: capsOnlyHint }} />
        {#if capsOnlyHint}<p class="inline-status">{t("ui.caps_lock_hint_hold_only")}</p>{/if}
      {/if}
      {#each [1, 4] as index}{@render appShortcut(index)}{/each}
    </section>
    <section class="shortcut-card" aria-labelledby="shortcuts-dictation-heading">
      {@render cardHeading("shortcuts-dictation-heading", "settings.dictation", "audio")}
      {#if ui.state?.platform !== "linux"}<Field spec={fields.recordingMode} />{/if}
      {#each [0, 2, 3] as index}{@render appShortcut(index)}{/each}
    </section>
    {#if portalShortcuts || reading.config}
      <section class="shortcut-card" aria-labelledby="shortcuts-reading-heading">
        {@render cardHeading("shortcuts-reading-heading", "settings.reading", "text")}
        {#each readerHotkeys as shortcut}
          {#if shortcut[1] !== "copy_region" && shortcut[1] !== "read_region"}{@render readerShortcut(shortcut)}{/if}
        {/each}
      </section>
      <section class="shortcut-card" aria-labelledby="shortcuts-image-heading">
        {@render cardHeading("shortcuts-image-heading", "module.ocr", "screen")}
        {#each readerHotkeys as shortcut}
          {#if shortcut[1] === "copy_region" || shortcut[1] === "read_region"}{@render readerShortcut(shortcut)}{/if}
        {/each}
      </section>
    {/if}
  </div>
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

<style>
  .shortcut-cards {
    display: grid;
    gap: 14px;
    margin-block-start: 18px;
  }

  .shortcut-card {
    min-inline-size: 0;
    padding: 0 12px 6px;
    border: 1px solid var(--frame);
    border-radius: var(--radius);
    background: var(--raised);
  }

  .shortcut-card h3 {
    display: flex;
    align-items: center;
    gap: 9px;
    margin: 0 -12px 4px;
    padding: 6px 12px;
    background: var(--title-bar);
    color: var(--title-ink);
    border-block-end: 1px solid var(--frame);
    border-radius: 2px 2px 0 0;
    font-size: 16px;
    font-weight: 650;
  }

  .shortcut-card h3 :global(svg) {
    color: inherit;
  }

  .shortcut-card :global(.field) {
    padding-block: 8px;
  }


  .shortcut-setup {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    white-space: nowrap;
    flex: 0 0 auto;
  }

  .shortcut-setup :global(svg) {
    flex: 0 0 auto;
  }

  @media (max-width: 500px) {
    :global(.group-head:has(.shortcut-setup)) {
      flex-wrap: wrap;
    }

    :global(.group-head:has(.shortcut-setup) .group-actions) {
      margin-inline-start: auto;
    }
  }
</style>
