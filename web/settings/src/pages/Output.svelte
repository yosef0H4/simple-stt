<script lang="ts">
  import {
    ui,
    set,
    markDirty,
    notice,
    platformAction,
  } from "../lib/state.svelte";
  import { fields, deliveryOptions } from "../lib/settings";
  import type { Backend, DeliveryMode } from "../lib/types";
  import Group from "../components/Group.svelte";
  import Field from "../components/Field.svelte";
  import Combo from "../components/Combo.svelte";
  import IconButton from "../components/IconButton.svelte";
  import Help from "../components/Help.svelte";
  import { t } from "../lib/i18n";
  import { positionToWpm, wpmToPosition } from "../lib/typing";
  let query = $state("");
  const typingPosition = $derived(ui.config?.output.paced_typing_enabled === false
    ? 100 : wpmToPosition(ui.config?.output.typing_speed_wpm ?? 450));
  const typingLabel = $derived(typingPosition === 100
    ? t("typing.instant") : `${ui.config?.output.typing_speed_wpm ?? 450}${t("ui.wpm.3fc2c6")}`);
  function setTypingPosition(event: Event) {
    const position = (event.currentTarget as HTMLInputElement).valueAsNumber;
    if (!Number.isFinite(position)) return;
    if (position < 100) set("output.typing_speed_wpm", positionToWpm(position));
    set("output.paced_typing_enabled", position < 100);
  }
  const linux = $derived(ui.state?.platform === "linux");
  const actionOrder: DeliveryMode[] = [
    "type", "smart_paste", "paste_ctrl_v", "paste_ctrl_shift_v",
    "paste_shift_insert", "clipboard",
  ];
  const choices = $derived(
    deliveryOptions
      .slice()
      .sort((a, b) => actionOrder.indexOf(a[0]) - actionOrder.indexOf(b[0]))
      .map(([mode, label]) => ({ mode, label, key: mode })),
  );
  const shown = $derived(
    choices.filter((c) =>
      `${t(c.label)} ${c.label}`.toLowerCase().includes(query.toLowerCase()),
    ),
  );
  function select(value: string) {
    set("output.delivery_mode", value as DeliveryMode);
    if (linux) {
      set("output.linux_automation_backend", "auto");
      normalizeLinuxCycle();
    }
  }
  function normalizeLinuxCycle() {
    if (!ui.config) return;
    const seen = new Set<DeliveryMode>();
    ui.config.output.linux_delivery_cycle = ui.config.output.linux_delivery_cycle
      .filter((entry) => {
        if (seen.has(entry.mode)) return false;
        seen.add(entry.mode);
        return true;
      })
      .map((entry) => ({ mode: entry.mode, backend: "auto" as Backend }));
  }
  function included(mode: DeliveryMode) {
    return linux
      ? ui.config?.output.linux_delivery_cycle.some((c) => c.mode === mode)
      : ui.config?.output.enabled_delivery_modes.includes(mode);
  }
  function toggle(mode: DeliveryMode) {
    if (!ui.config) return;
    const output = ui.config.output;
    if (linux) {
      normalizeLinuxCycle();
      if (included(mode)) {
        if (output.linux_delivery_cycle.length === 1)
          return notice(t("ui.keep_at_least_one_delivery.2d7d41"), "error");
        output.linux_delivery_cycle = output.linux_delivery_cycle.filter(
          (c) => c.mode !== mode,
        );
      } else output.linux_delivery_cycle.push({ mode, backend: "auto" });
      set("output.linux_automation_backend", "auto");
    } else {
      if (included(mode)) {
        if (output.enabled_delivery_modes.length === 1)
          return notice(t("ui.keep_at_least_one_delivery.2d7d41"), "error");
        output.enabled_delivery_modes = output.enabled_delivery_modes.filter(
          (m) => m !== mode,
        );
      } else output.enabled_delivery_modes.push(mode);
    }
    markDirty();
  }
  async function addCurrent() {
    const result = await platformAction("focused_app");
    if (
      result.app_id &&
      !ui.config?.output.app_overrides.some((e) => e.app_id === result.app_id)
    ) {
      ui.config?.output.app_overrides.push({
        app_id: result.app_id,
        mode: "smart_paste",
      });
      markDirty();
    }
  }
</script>

<Group
  title="ui.delivery.9631af"
  icon="output"
  paths={[
    "output.delivery_mode",
    "output.enabled_delivery_modes",
    "output.preserve_clipboard",
  ]}
>
  <Combo
    label="ui.use_now.36690a"
    path="output.delivery_mode"
    ariaLabel={t(linux ? "Current Linux delivery method" : "Current Windows delivery method")}
    value={`${ui.config?.output.delivery_mode}`}
    items={choices.map((c) => ({
      value: c.key,
      label: t(c.label),
    }))}
    onselect={select}
  />
  <Field spec={fields.preserveClipboard} />
  <details class="delivery-picker-advanced">
    <summary>{t("ui.cycle_between.75667a")}</summary>
    <div
      data-setting-path={linux
        ? "output.linux_delivery_cycle"
        : "output.enabled_delivery_modes"}
    >
      <input
        type="search"
        placeholder={t("ui.search_tools_and_delivery_methods.717e7b")}
        aria-label={t("ui.search_tools_and_delivery_methods.717e7b")}
        bind:value={query}
      />{#each shown as c}<label class="cycle-row"
          ><span
            >{t(c.label)}</span
          ><input
            type="checkbox"
            aria-label={t("ui.include_method_in_delivery_cycle.d45eee", { method: t(c.label) })}
            checked={included(c.mode)}
            onchange={() => toggle(c.mode)}
          /></label
        >{/each}
    </div>
  </details>
</Group>
<Group
  title="ui.typing.5614fd"
  icon="keyboard"
  paths={["output.paced_typing_enabled", "output.typing_speed_wpm"]}
  ><div data-setting-path="output.paced_typing_enabled">
    <div class="field" data-setting-path="output.typing_speed_wpm">
      <div class="field-copy"><label for="typing-speed">{t("ui.speed.2d2cb0")}</label><Help text={t("typing.speed_help")} /></div>
      <div class="control">
        <input id="typing-speed" type="range" min="0" max="100" step="1" dir="ltr"
          aria-valuetext={typingLabel} value={typingPosition} oninput={setTypingPosition} />
        <input class="range-number" type="number" aria-label={t("settings.typing_exact")} min="50" max="1650" step="1" value={typingPosition===100?"":ui.config?.output.typing_speed_wpm} placeholder={t("typing.instant")} oninput={event=>{const value=event.currentTarget.valueAsNumber;if(Number.isFinite(value)){set("output.typing_speed_wpm",value);set("output.paced_typing_enabled",true);}}}/><output class="range-value" for="typing-speed">{typingPosition===100?t("typing.instant"):t("ui.wpm.3fc2c6")}</output>
      </div>
    </div>
  </div></Group
>
<Group title="ui.app_overrides.9e577a" icon="apps" paths={["output.app_overrides"]}>
  {#snippet actions()}<IconButton
      icon="plus"
      label="ui.add_manually.a1d9c2"
      onclick={() => {
        ui.config?.output.app_overrides.push({
          app_id: "",
          mode: "smart_paste",
        });
        markDirty();
      }}
    /><IconButton
      icon="output"
      label="ui.add_current_app.4321a4"
      title="ui.add_current_app_focus_it.319c40"
      onclick={addCurrent}
    />{/snippet}
  <div data-setting-path="output.app_overrides">
    {#each ui.config?.output.app_overrides || [] as entry, index}<div
        class="app-override-row"
      >
        <input
          aria-label={t("ui.application_identity.74e9a6")}
          value={entry.app_id}
          placeholder={t("ui.application_id.557031")}
          oninput={(e) => {
            entry.app_id = e.currentTarget.value;
            markDirty();
          }}
        /><select
          aria-label={t("ui.delivery_mode_for_app.763ef6", { app: entry.app_id || t("ui.application.d2005c") })}
          value={entry.mode}
          onchange={(e) => {
            entry.mode = e.currentTarget.value as DeliveryMode;
            markDirty();
          }}
          >{#each deliveryOptions as [value, label]}<option {value}
              >{t(label)}</option
            >{/each}</select
        ><IconButton
          icon="trash"
          label="ui.remove_app_override.10d229"
          onclick={() => {
            ui.config?.output.app_overrides.splice(index, 1);
            markDirty();
          }}
        />
      </div>{:else}<p class="empty">{t("ui.no_overrides.473cfd")}</p>{/each}
  </div>
</Group>
<Group
  title="ui.text.c3328c"
  icon="text"
  paths={[
    "output.trailing_space",
    "output.remove_punctuation",
    "output.lowercase",
  ]}
  >{#each ["space", "punctuation", "lowercase"] as key}<Field
      spec={fields[key]}
    />{/each}</Group
>
