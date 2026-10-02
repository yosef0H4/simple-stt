<script lang="ts">
  import { onDestroy } from "svelte";
  import {
    ui,
    cleanupAction as requestCleanupAction,
    refreshState as requestRefresh,
    set,
    get,
    delay,
  } from "../lib/state.svelte";
  import { fields } from "../lib/settings";
  import Group from "../components/Group.svelte";
  import Field from "../components/Field.svelte";
  import Combo from "../components/Combo.svelte";
  import IconButton from "../components/IconButton.svelte";
  import type { Reasoning } from "../lib/types";
  let key = $state(""),
    test = $state("uh hello jason no sorry Jayson comma this is a test"),
    result = $state(""),
    testing = $state(false);
  const connected = $derived(ui.state?.cleanup?.chatgpt_connected || false);
  const chatgpt = $derived(ui.config?.cleanup.provider === "chat_gpt");
  const modelPath = $derived(
    chatgpt ? "cleanup.chatgpt.model" : "cleanup.openai_compatible.model",
  );
  const reasoningPath = $derived(
    chatgpt
      ? "cleanup.chatgpt.reasoning_effort"
      : "cleanup.openai_compatible.reasoning_effort",
  );
  const controller = new AbortController();
  onDestroy(() => controller.abort());
  const cleanupAction = (action: string, extra: Record<string, unknown> = {}) =>
    requestCleanupAction(action, extra, controller.signal);
  const refreshState = () => requestRefresh(false, controller.signal);
  let provider = "";
  $effect(() => {
    const selected = ui.config?.cleanup.provider || "";
    if (provider && provider !== selected) ui.cleanupModels = [];
    provider = selected;
  });
  async function login(code = false) {
    const response = await cleanupAction(
      code ? "chatgpt_login_code" : "chatgpt_login_browser",
    );
    if (code && response.code)
      await navigator.clipboard.writeText(response.code);
    if (response.url) window.open(response.url, "_blank", "noopener");
    for (let i = 0; i < 150 && !controller.signal.aborted; i++) {
      await delay(2000, controller.signal);
      if (controller.signal.aborted) return;
      await refreshState();
      if (
        ui.state?.cleanup?.chatgpt_connected ||
        ui.state?.cleanup?.auth_status?.state === "error"
      )
        return;
    }
  }
  async function testCleanup() {
    testing = true;
    try {
      result =
        (await cleanupAction("test", { transcript: test })).result?.text || "";
    } catch (e) {
      result = String(e);
    } finally {
      testing = false;
    }
  }
</script>

<Group title="AI cleanup" paths={["cleanup.enabled", "cleanup.provider"]}
  ><Field spec={fields.cleanup} /><Field spec={fields.provider} /></Group
>
<Group
  title="Connection"
  paths={["cleanup.openai_compatible", "cleanup.chatgpt"]}
>
  {#if !chatgpt}<Field spec={fields.url} />
    <div class="field" data-setting-path="cleanup.secret">
      <label for="api-key">API key</label>
      <div class="control">
        <input
          id="api-key"
          type="password"
          aria-label="Provider API key"
          autocomplete="new-password"
          placeholder={ui.state?.cleanup?.compatible_key_saved
            ? "Replace saved key"
            : "Paste API key"}
          bind:value={key}
        /><IconButton
          icon="check"
          label="Save API key"
          disabled={!key.trim()}
          onclick={async () => {
            await cleanupAction("save_api_key", { secret: key });
            key = "";
            await refreshState();
          }}
        />{#if ui.state?.cleanup?.compatible_key_saved}<IconButton
            icon="trash"
            label="Remove saved API key"
            onclick={async () => {
              await cleanupAction("delete_api_key");
              await refreshState();
            }}
          />{/if}
      </div>
    </div>
  {:else}<div class="field">
      <span>ChatGPT</span>
      <div class="control">
        <span class="account-status"
          >{connected ? "Connected" : "Not connected"}</span
        >{#if connected}<IconButton
            icon="close"
            label="Disconnect ChatGPT"
            onclick={async () => {
              await cleanupAction("chatgpt_logout");
              await refreshState();
            }}
          />{:else}<IconButton
            icon="connect"
            label="Connect ChatGPT"
            onclick={() => login()}
          /><IconButton
            icon="copy"
            label="Connect using a code"
            onclick={() => login(true)}
          />{/if}
      </div>
    </div>
    {#if ui.state?.cleanup?.auth_status?.code}<code
        >{ui.state.cleanup.auth_status.code}</code
      >{/if}{/if}
  <Combo
    label="Cleanup model"
    path={modelPath}
    value={String(get(modelPath) || "")}
    items={ui.cleanupModels.map((m) => ({ value: m.id, label: m.id }))}
    custom
    onselect={(v) => set(modelPath, v)}
    help="Fetch models or enter the provider’s exact model ID."
  >
    {#snippet extra()}<IconButton
        icon="refresh"
        label="Fetch models"
        onclick={async () =>
          (ui.cleanupModels =
            (await cleanupAction("list_models")).models || [])}
      />{/snippet}</Combo
  >
</Group>
<Group
  title="Text cleanup"
  paths={[
    "cleanup.prompt",
    "cleanup.timeout_ms",
    "cleanup.max_output_tokens",
    "cleanup.openai_compatible.reasoning_effort",
    "cleanup.chatgpt.reasoning_effort",
  ]}
>
  <Field
    spec={{
      path: reasoningPath,
      label: "Reasoning",
      type: "select",
      options: [
        ["none", "None"],
        ["low", "Low"],
        ["medium", "Medium"],
        ["high", "High"],
        ["xhigh", "Extra high"],
        ["max", "Maximum"],
      ] as [Reasoning, string][],
    }}
  />
  {#each ["timeout", "tokens", "prompt"] as key}<Field
      spec={fields[key]}
    />{/each}
  <div class="cleanup-test">
    <textarea aria-label="Test transcript" rows="3" bind:value={test}
    ></textarea><IconButton
      icon="test"
      label="Test cleanup"
      disabled={testing}
      onclick={testCleanup}
    />{#if result}<output>{result}</output>{/if}
  </div>
</Group>
<Group title="Screen context" paths={["cleanup.screenshot"]}>
  {#each ["screenshot", "scope", "imageSize", "quality"] as key}<Field
      spec={{ ...fields[key], disabled: !ui.config?.cleanup.enabled }}
    />{/each}
  <div class="field" data-setting-path="cleanup.screenshot.excluded_apps">
    <label for="excluded-apps">Never capture</label><textarea
      id="excluded-apps"
      rows="3"
      disabled={!ui.config?.cleanup.enabled}
      value={ui.config?.cleanup.screenshot.excluded_apps.join("\n") || ""}
      placeholder="One application per line"
      oninput={(e) =>
        set(
          "cleanup.screenshot.excluded_apps",
          e.currentTarget.value
            .split(/\r?\n/)
            .map((s) => s.trim())
            .filter(Boolean),
        )}></textarea>
  </div>
</Group>
<Group title="Recent cleanup">
  {#snippet actions()}<IconButton
      icon="trash"
      label="Clear cleanup history"
      disabled={!ui.state?.cleanup?.history.length}
      onclick={async () => {
        await cleanupAction("clear_history");
        await refreshState();
      }}
    />{/snippet}
  {#each ui.state?.cleanup?.history || [] as entry}<article class="history-row">
      <small
        >{entry.model} · {entry.latency_ms} ms · {entry.outcome === "cleaned"
          ? "Cleaned"
          : "Original used"}</small
      >
      <details>
        <summary>Original</summary>
        <p dir="auto">{entry.raw}</p>
      </details>
      <p dir="auto">{entry.cleaned}</p>
      <IconButton
        icon="copy"
        label="Copy cleaned text"
        onclick={() => navigator.clipboard.writeText(entry.cleaned)}
      />
    </article>{:else}<p class="empty">No cleaned dictation yet</p>{/each}
</Group>
