<script lang="ts">
  import { onMount, tick } from "svelte";
  import { ui, load, eventLoop, save, run, dispose, refreshAutomaticSelections } from "./lib/state.svelte";
  import {catalogView,refreshModules,onAutomaticSelection} from "./lib/modules.svelte";
  import { api, hasToken, setToken } from "./lib/api";
  import { pages, searchEntries, fuzzyScore } from "./lib/settings";
  import Icon from "./components/Icon.svelte";
  import IconButton from "./components/IconButton.svelte";
  import Reading from "./pages/Reading.svelte";
  import General from "./pages/General.svelte";
  import Audio from "./pages/Audio.svelte";
  import Models from "./pages/Models.svelte";
  import Cleanup from "./pages/Cleanup.svelte";
  import Advanced from "./pages/Advanced.svelte";
  import type { Page } from "./lib/types";
  import { direction, locale, t, tEnglish } from "./lib/i18n";
  let query = $state(""),
    searchIndex = $state(0);
  let authenticated = $state(hasToken());
  let signInToken = $state("");
  let signInError = $state("");
  let signingIn = $state(false);
  let stopSession: (() => void) | undefined;
  const results = $derived(
    query.trim()
      ? searchEntries
          .filter(
            (e) =>
              ui.state?.platform === "linux" ||
              e.path !== "diagnostics.clipboard_buffer_notices",
          )
          .map((e) => ({
            ...e,
            score: fuzzyScore(query, `${t(e.label)} ${tEnglish(e.label)} ${e.path} ${t(pages.find(page=>page.id===e.page)?.name||"")} ${"help" in e && e.help ? t(e.help):""} ${"aliases" in e ? e.aliases : ""}`),
          }))
          .filter((e) => e.score >= 0)
          .sort((a, b) => b.score - a.score)
          .slice(0, 7)
      : [],
  );
  const current = $derived(pages.find((p) => p.id === ui.page) || pages[0]);
  function startSession() {
    onAutomaticSelection(refreshAutomaticSelections);
    const refreshTimer=setInterval(()=>{void refreshModules().catch(()=>{});},2000);
    void refreshModules().catch(()=>{});
    const controller = new AbortController();
    void run(async () => {
      await load();
      if (!controller.signal.aborted && ui.state?.service_online)
        void eventLoop(controller.signal);
    });
    return () => {
      onAutomaticSelection();
      clearInterval(refreshTimer);
      controller.abort();
      dispose();
    };
  }
  onMount(() => {
    if (authenticated) stopSession = startSession();
    return () => stopSession?.();
  });
  async function signIn(event: SubmitEvent) {
    event.preventDefault();
    signingIn = true;
    signInError = "";
    setToken(signInToken);
    try {
      await api("/api/state");
      signInToken = "";
      authenticated = true;
      stopSession = startSession();
    } catch {
      setToken("");
      signInError = t("admin.sign_in_failed");
    } finally { signingIn = false; }
  }
  $effect(() => {
    document.documentElement.dataset.theme =
      ui.config?.general.ui_theme || "auto";
    document.documentElement.lang = locale();
    document.documentElement.dir = direction();
    document.title = `${t("ui.settings.c7f73b")} · Simple STT`;
  });
  async function jump(page: Page, path = "") {
    if (page === "models") {
      catalogView.panel=path.startsWith("cleanup.")?"cleanup":"models";
      if(path.startsWith("tts."))catalogView.feature="tts";
      else if(path.startsWith("ocr."))catalogView.feature="ocr";
      else if(path.startsWith("speech."))catalogView.feature="stt";
    }
    ui.page = page;
    query = "";
    await tick();
    window.scrollTo({ top: 0, left: 0, behavior: "instant" });
    if (path) {
      const field = document.querySelector(`[data-setting-path="${path}"]`) || document.getElementById(path) || document.querySelector(`[data-setting-path$=".${path.split('.').at(-1)}"]`);
      if (field) {
        for (let p = field.parentElement; p; p = p.parentElement)
          if (p instanceof HTMLDetailsElement) p.open = true;
        field.scrollIntoView({ block: "center" });
        (
          (field.querySelector("input,select,textarea") ||
            field.querySelector("button")) as HTMLElement
        )?.focus();
      }
    }
  }
  function searchKey(event: KeyboardEvent) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      searchIndex = Math.min(results.length - 1, searchIndex + 1);
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      searchIndex = Math.max(0, searchIndex - 1);
    }
    if (event.key === "Enter" && results[searchIndex]) {
      event.preventDefault();
      void jump(results[searchIndex].page, results[searchIndex].path);
    }
    if (event.key === "Escape") query = "";
  }
  async function close() {
    await api("/api/close", {});
    window.close();
  }
</script>

<svelte:window
  onbeforeunload={(event) => {
    if (ui.dirty) {
      event.preventDefault();
      event.returnValue = "";
    }
  }}
/>
{#if !authenticated}
  <main class="admin-sign-in">
    <h1>Simple STT</h1>
    <p>{t("admin.sign_in_help")}</p>
    <form onsubmit={signIn}>
      <label for="admin-token">{t("admin.token")}</label>
      <input id="admin-token" type="password" autocomplete="current-password" bind:value={signInToken} required disabled={signingIn} />
      <button type="submit" disabled={signingIn || !signInToken.trim()}>{t("admin.sign_in")}</button>
      {#if signInError}<p role="alert">{signInError}</p>{/if}
    </form>
  </main>
{:else}
<div class="app-shell">
  <aside class="rail">
    <div class="brand">
      <span class="brand-mark"><Icon name="audio" size={23} /></span><strong
        >Simple STT</strong
      >
    </div>
    <div class="settings-search">
      <Icon name="search" size={17} /><input
        id="settings-search"
        type="search"
        role="combobox"
        aria-autocomplete="list"
        aria-label={t("ui.search_all_settings.46e7c7")}
        aria-controls="settings-search-results"
        aria-expanded={results.length > 0}
        aria-activedescendant={results[searchIndex]
          ? `setting-result-${searchIndex}`
          : undefined}
        placeholder={t("ui.find_a_setting.fff44d")}
        bind:value={query}
        oninput={() => (searchIndex = 0)}
        onkeydown={searchKey}
      />{#if results.length}<div
          id="settings-search-results"
          class="settings-search-results"
          role="listbox"
          aria-label={t("ui.settings_search.b702dd")}
        >
          {#each results as result, i}<button
              type="button"
              role="option"
              id={`setting-result-${i}`}
              aria-selected={i === searchIndex}
              onclick={() => jump(result.page, result.path)}
              >{t(result.label)}<small
                >{t(pages.find((p) => p.id === result.page)?.name || "")}</small
              ></button
            >{/each}
        </div>{/if}
    </div>
    <nav aria-label={t("ui.settings_pages.032a4a")}>
      {#each pages as page}<button
          type="button"
          data-page={page.id}
          aria-label={t(page.name)}
          title={t(page.name)}
          aria-current={ui.page === page.id ? "page" : undefined}
          class:active={ui.page === page.id}
          onclick={() => jump(page.id)}
          ><Icon name={page.icon} /><span class="nav-copy"><span>{t(page.name)}</span><small>{t(page.description)}</small></span></button
        >{/each}
    </nav>
    <div class="rail-bottom">
      <span
        id="service"
        class="service-pill"
        data-state={ui.state?.service_online ? "online" : "offline"}
        title={ui.state?.service_online
          ? t("ui.capture_service_connected.45ad0f")
          : t("ui.offline_editing.aba8c0")}
        ><i></i><span>{t(ui.state?.service_online ? "Connected" : "Offline")}</span
        ></span
      ><IconButton icon="close" label="ui.close_settings.5bd91a" onclick={close} />
    </div>
  </aside>
  <main>
    <header class="page-head"><h1>{t(current.name)}</h1><p class="page-description">{t(current.description)}</p></header>
    <form
      id="settings"
      inert={ui.configLoading}
      aria-busy={ui.configLoading}
      onsubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      {#if ui.ready}<section data-section={ui.page} class="page-content">
          {#if ui.page === "general"}<General
            />{:else if ui.page === "audio"}<Audio
            />{:else if ui.page === "models"}<Models
            />{:else if ui.page === "reading"}<Reading />{:else if ui.page === "cleanup"}<Cleanup />{:else}<Advanced />{/if}
        </section>{:else}<div
          class="loading-skeleton"
          aria-label={t("ui.configLoading_settings.2ee974")}
        >
          <span></span><span></span><span></span>
        </div>{/if}
    </form>
  </main>
</div>
{#if ui.notice}<div
    id="notice"
    class="notice"
    data-kind={ui.noticeKind}
    role="status"
    aria-live="polite"
  >
    <span>{ui.notice}</span>{#if ui.noticeDetails}<details class="notice-detail" open><summary>{t("ui.details.dc3dec")}</summary><div dir="auto">{ui.noticeDetails}</div></details>{/if}
  </div>{/if}
{#if ui.dirty}<footer id="savebar" class="savebar">
    <span id="dirty"><i></i>{t("ui.unsaved_changes.292672")}</span><button
      type="submit"
      form="settings"
      class="primary"
      disabled={ui.saving}>{t(ui.saving ? "Saving…" : "Save")}</button
    >
  </footer>{/if}
{/if}
