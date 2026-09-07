import { defineConfig } from "vite";

import pkg from "./package.json" with { type: "json" };

// Vite config tuned for Tauri: fixed dev port, no clearing of the Rust logs,
// and a build target the WebView2 runtime understands.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  // The version, so the browser fallback in main.js need not hand-carry a
  // fourth copy of it alongside package.json, Cargo.toml and tauri.conf.json.
  define: { __APP_VERSION__: JSON.stringify(pkg.version) },
  build: {
    target: "chrome105",
    minify: process.env.TAURI_ENV_DEBUG ? false : "esbuild",
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});
