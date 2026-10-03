import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { loadEnv } from "vite";
const origin = loadEnv("development", ".", "").SIMPLE_STT_SETTINGS_ORIGIN;
export default defineConfig({
  plugins: [svelte()],
  server: {
    host: "127.0.0.1",
    proxy: origin
      ? {
          "/api": {
            target: origin,
            changeOrigin: true,
            configure(proxy) {
              proxy.on("proxyReq", (request) =>
                request.setHeader("Origin", origin),
              );
            },
          },
        }
      : undefined,
  },
  build: {
    target: "es2022",
    cssCodeSplit: false,
    assetsInlineLimit: 0,
    rolldownOptions: {
      output: {
        entryFileNames: "app.js",
        assetFileNames: (asset) => asset.names.some((name) => name.endsWith(".woff2")) ? "arabic.woff2" : "styles.css",
        codeSplitting: false,
      },
    },
  },
});
