<script lang="ts">
  import {
    ui,
    set,
    markDirty,
    notice,
    platformAction,
    refreshState,
  } from "../lib/state.svelte";
  import { fields, deliveryOptions } from "../lib/settings";
  import type { Backend, DeliveryMode } from "../lib/types";
  import Group from "../components/Group.svelte";
  import Field from "../components/Field.svelte";
  import Combo from "../components/Combo.svelte";
  import IconButton from "../components/IconButton.svelte";
  import { t } from "../lib/i18n";
  let query = $state("");
  const tools = $derived(ui.state?.linux_automation || {});
  const linux = $derived(ui.state?.platform === "linux");
  const backends = $derived<[Backend, string, boolean][]>([
    ["auto", "Automatic", true],
    ["native", "Native fast paste", Boolean(tools.native)],
    ["wtype", "wtype", Boolean(tools.wtype)],
    ["ydotool", "ydotool", Boolean(tools.ydotool && tools.ydotool_daemon)],
    ["xdotool", "xdotool", Boolean(tools.xdotool)],
    ["clipboard_only", "wl-clipboard", Boolean(tools.wl_clipboard)],
  ]);
  const choices = $derived(
    deliveryOptions.flatMap(([mode, label]) =>
      (linux
        ? backends
        : [["auto", "AutoHotkey", true] as [Backend, string, boolean]]
      )
        .filter(([backend]) =>
          backend === "clipboard_only"
            ? mode === "clipboard"
            : backend === "native"
              ? mode !== "type"
              : true,
        )
        .map(([backend, tool, installed]) => ({
          mode,
          backend,
          label,
          tool,
          installed,
          key: `${backend}|${mode}`,
        })),
    ),
  );
  const shown = $derived(
    choices.filter((c) =>
      `${t(c.label)} ${t(c.tool)} ${c.label} ${c.tool}`.toLowerCase().includes(query.toLowerCase()),
    ),
  );
  function select(value: string) {
    const [backend, mode] = value.split("|") as [Backend, DeliveryMode];
    set("output.delivery_mode", mode);
    if (linux) set("output.linux_automation_backend", backend);
  }
  function included(mode: DeliveryMode, backend: Backend) {
    return linux
      ? ui.config?.output.linux_delivery_cycle.some(
          (c) => c.mode === mode && c.backend === backend,
        )
      : ui.config?.output.enabled_delivery_modes.includes(mode);
  }
  function toggle(mode: DeliveryMode, backend: Backend) {
    if (!ui.config) return;
    const output = ui.config.output;
    if (linux) {
      if (included(mode, backend)) {
        if (output.linux_delivery_cycle.length === 1)
          return notice(t("ui.keep_at_least_one_delivery.2d7d41"), "error");
        output.linux_delivery_cycle = output.linux_delivery_cycle.filter(
          (c) => c.mode !== mode || c.backend !== backend,
        );
      } else output.linux_delivery_cycle.push({ mode, backend });
    } else {
      if (included(mode, backend)) {
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
  const packages = $derived(
    tools.session === "X11"
      ? "xdotool xclip"
      : String(tools.desktop).toLowerCase().includes("kde")
        ? "wl-clipboard"
        : String(tools.desktop).toLowerCase().includes("gnome")
          ? "wl-clipboard ydotool"
          : "wl-clipboard wtype",
  );
  const install = $derived(
    tools.distro_id === "fedora"
      ? `sudo dnf install ${packages}`
      : ["arch", "manjaro", "endeavouros"].includes(tools.distro_id || "")
        ? `sudo pacman -S ${packages}`
        : ["debian", "ubuntu", "linuxmint", "pop"].includes(
              tools.distro_id || "",
            )
          ? `sudo apt install ${packages}`
          : `Install ${packages} with your package manager.`,
  );
</script>

<Group
  title="ui.delivery.9631af"
  icon="output"
  paths={[
    "output.delivery_mode",
    "output.enabled_delivery_modes",
    "output.linux_automation_backend",
    "output.linux_delivery_cycle",
    "output.preserve_clipboard",
  ]}
>
  {#snippet actions()}{#if linux}<IconButton
        icon="refresh"
        label="ui.refresh_tools.311c38"
        onclick={() => refreshState()}
      />{/if}{/snippet}
  <Combo
    label="ui.use_now.36690a"
    path="output.delivery_mode"
    ariaLabel={t(linux ? "Current Linux delivery method" : "Current Windows delivery method")}
    value={`${linux ? ui.config?.output.linux_automation_backend : "auto"}|${ui.config?.output.delivery_mode}`}
    items={choices.map((c) => ({
      value: c.key,
      label: `${t(c.label)}${linux ? ` · ${c.tool}` : ""}${c.installed ? "" : ` · ${t("ui.not_installed.1aab9f")}`}`,
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
            >{t(c.label)}{#if linux}<small dir="ltr"
                >{c.tool}{c.installed ? "" : ` · ${t("ui.not_installed.1aab9f")}`}</small
              >{/if}</span
          ><input
            type="checkbox"
            aria-label={t("ui.include_method_in_delivery_cycle.d45eee", { method: `${t(c.label)}${linux ? ` ${c.tool}` : ""}` })}
            checked={included(c.mode, c.backend)}
            onchange={() => toggle(c.mode, c.backend)}
          /></label
        >{/each}
    </div>
  </details>
  {#if linux}<details class="guide">
      <summary>{t("ui.tool_setup.f7caa3")}</summary>
      <p>{tools.distro} · {tools.desktop} · {tools.session}</p>
      <p>{t("ui.automatic.ac9041")}: {tools.recommended || t("ui.not_available.d1a17a")}</p>
      <code>{install}</code>
        <p>{t("ui.wl_clipboard_handles_clipboard_data.68c501")}</p>
    </details>{/if}
</Group>
<Group
  title="ui.typing.5614fd"
  icon="keyboard"
  paths={["output.paced_typing_enabled", "output.typing_speed_wpm"]}
  ><Field spec={fields.paced} /><Field
    spec={{
      ...fields.speed,
      disabled: !ui.config?.output.paced_typing_enabled,
    }}
  /></Group
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
              >{label}</option
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
