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
      `${c.label} ${c.tool}`.toLowerCase().includes(query.toLowerCase()),
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
          return notice("Keep at least one delivery choice.", "error");
        output.linux_delivery_cycle = output.linux_delivery_cycle.filter(
          (c) => c.mode !== mode || c.backend !== backend,
        );
      } else output.linux_delivery_cycle.push({ mode, backend });
    } else {
      if (included(mode, backend)) {
        if (output.enabled_delivery_modes.length === 1)
          return notice("Keep at least one delivery choice.", "error");
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
  title="Delivery"
  icon="output"
  paths={[
    "output.delivery_mode",
    "output.enabled_delivery_modes",
    "output.linux_automation_backend",
    "output.linux_delivery_cycle",
  ]}
>
  {#snippet actions()}{#if linux}<IconButton
        icon="refresh"
        label="Refresh tools"
        onclick={() => refreshState()}
      />{/if}{/snippet}
  <Combo
    label="Use now"
    path="output.delivery_mode"
    ariaLabel={`Current ${linux ? "Linux" : "Windows"} delivery method`}
    value={`${linux ? ui.config?.output.linux_automation_backend : "auto"}|${ui.config?.output.delivery_mode}`}
    items={choices.map((c) => ({
      value: c.key,
      label: `${c.label}${linux ? ` · ${c.tool}` : ""}${c.installed ? "" : " · Not installed"}`,
    }))}
    onselect={select}
  />
  <details class="delivery-picker-advanced">
    <summary>Cycle between</summary>
    <div
      data-setting-path={linux
        ? "output.linux_delivery_cycle"
        : "output.enabled_delivery_modes"}
    >
      <input
        type="search"
        placeholder="Search tools and delivery methods"
        aria-label="Search tools and delivery methods"
        bind:value={query}
      />{#each shown as c}<label class="cycle-row"
          ><span
            >{c.label}{#if linux}<small
                >{c.tool}{c.installed ? "" : " · Not installed"}</small
              >{/if}</span
          ><input
            type="checkbox"
            aria-label={`Include ${c.label}${linux ? ` ${c.tool}` : ""} in delivery cycle`}
            checked={included(c.mode, c.backend)}
            onchange={() => toggle(c.mode, c.backend)}
          /></label
        >{/each}
    </div>
  </details>
  {#if linux}<details class="guide">
      <summary>Tool setup</summary>
      <p>{tools.distro} · {tools.desktop} · {tools.session}</p>
      <p>Automatic: {tools.recommended || "Not available"}</p>
      <code>{install}</code>
      <p>
        wl-clipboard handles clipboard data. wtype supports virtual-keyboard
        Wayland desktops. ydotool requires ydotoold and input-device access.
        xdotool is for X11. Native fast paste supports compositor-aware paste.
      </p>
    </details>{/if}
</Group>
<Group
  title="Typing"
  icon="keyboard"
  paths={["output.paced_typing_enabled", "output.typing_speed_wpm"]}
  ><Field spec={fields.paced} /><Field
    spec={{
      ...fields.speed,
      disabled: !ui.config?.output.paced_typing_enabled,
    }}
  /></Group
>
<Group title="App overrides" icon="apps" paths={["output.app_overrides"]}>
  {#snippet actions()}<IconButton
      icon="plus"
      label="Add manually"
      onclick={() => {
        ui.config?.output.app_overrides.push({
          app_id: "",
          mode: "smart_paste",
        });
        markDirty();
      }}
    /><IconButton
      icon="output"
      label="Add current app"
      title="Add current app · focus it within 3 seconds"
      onclick={addCurrent}
    />{/snippet}
  <div data-setting-path="output.app_overrides">
    {#each ui.config?.output.app_overrides || [] as entry, index}<div
        class="app-override-row"
      >
        <input
          aria-label="Application identity"
          value={entry.app_id}
          placeholder="Application ID"
          oninput={(e) => {
            entry.app_id = e.currentTarget.value;
            markDirty();
          }}
        /><select
          aria-label={`Delivery mode for ${entry.app_id || "application"}`}
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
          label="Remove app override"
          onclick={() => {
            ui.config?.output.app_overrides.splice(index, 1);
            markDirty();
          }}
        />
      </div>{:else}<p class="empty">No overrides</p>{/each}
  </div>
</Group>
<Group
  title="Text"
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
