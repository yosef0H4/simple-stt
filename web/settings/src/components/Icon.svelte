<script lang="ts">
  let { name, size = 18 }: { name: string; size?: number } = $props();
  const paths: Record<string, string> = {
    refresh:
      "M20 6v5h-5M4 18v-5h5 M18.5 9A7 7 0 0 0 6 6.5L4 9m2 6a7 7 0 0 0 12 2.5L20 15",
    reset: "M3 12a9 9 0 1 0 3-6.7L3 8 M3 3v5h5",
    download: "M12 3v12m-5-5 5 5 5-5M5 21h14",
    check: "m5 12 4 4L19 6",
    test: "m9 3-5 9h7l-1 9 10-12h-7l1-6Z",
    trash: "M3 6h18M8 6V4h8v2m3 0-1 15H6L5 6m5 4v7m4-7v7",
    connect:
      "M10 13a5 5 0 0 0 7.5.5l2-2a5 5 0 0 0-7-7l-1 1M14 11a5 5 0 0 0-7.5-.5l-2 2a5 5 0 0 0 7 7l1-1",
    copy: "M8 8h12v12H8zM4 16V4h12",
    close: "M6 6l12 12M18 6 6 18",
    help: "M9 9a3 3 0 1 1 5 2c-1 1-2 1-2 3m0 3h.01 M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0",
    chevron: "m8 10 4 4 4-4",
    search: "M21 21l-5-5M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0",
    general: "M4 3v18M12 3v18M20 3v18M1 8h6M9 16h6M17 9h6",
    audio:
      "M9 5a3 3 0 0 1 6 0v7a3 3 0 0 1-6 0V5 M5 10v2a7 7 0 0 0 14 0v-2M12 19v3m-4 0h8",
    models: "M12 3l9 5-9 5-9-5 9-5Zm-9 9 9 5 9-5m-18 5 9 5 9-5",
    output: "M3 12h15m-5-5 5 5-5 5M21 5v14",
    cleanup: "m4 20 12-12m-9 5 4 4M17 2v4m-2-2h4M21 9v4m-2-2h4M8 2v4M6 4h4",
    advanced:
      "M12 8a4 4 0 1 1 0 8 4 4 0 0 1 0-8M12 2v3m0 14v3M2 12h3m14 0h3M5 5l2 2m10 10 2 2M5 19l2-2M17 7l2-2",
    config: "M14 2H5v20h14V7l-5-5Zm0 0v5h5M8 12h8m-8 4h8",
    plus: "M12 4v16M4 12h16",
    folder: "M3 5h7l2 3h9v12H3V5Z",
    upload: "M12 17V3m-5 5 5-5 5 5M5 21h14",
    keyboard:
      "M3 5h18v14H3V5Zm3 4h1m3 0h1m3 0h1m3 0h1M6 13h1m3 0h1m3 0h4M7 16h10",
    system: "M12 3v9M8 5a8 8 0 1 0 8 0",
    recognition:
      "M5 5h14v14H5V5Zm4 4h6v6H9V9ZM9 2v3m6-3v3M9 19v3m6-3v3M2 9h3m-3 6h3m14-6h3m-3 6h3",
    apps: "M3 3h8v8H3V3Zm10 0h8v8h-8V3ZM3 13h8v8H3v-8Zm10 0h8v8h-8v-8Z",
    text: "M4 5h16M12 5v14m-4 0h8",
    screen: "M3 4h18v14H3V4Zm5 18h8m-4-4v4M7 8h10m-10 4h6",
    history: "M3 11a9 9 0 1 1 2 7M3 4v7h7M12 7v5l3 2",
    diagnostics: "M3 4h18v16H3V4Zm3 4 3 3-3 3m6 1h6",
  };
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.7"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"><path d={paths[name] || paths.general} /></svg
>
