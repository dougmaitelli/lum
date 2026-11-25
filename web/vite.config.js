import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Keep Host and Origin untouched. The Rust dev backend validates the browser's
// loopback authority, exactly as it does when serving the production bundle.
const target = process.env.LUM_DEV_BACKEND;
export default defineConfig({
  plugins: [svelte()],
  server: {
    host: "127.0.0.1",
    strictPort: true,
    cors: false,
    fs: { allow: [import.meta.dirname] },
    proxy: target
      ? Object.fromEntries(["/admin/", "/metrics"].map((p) => [p, { target, changeOrigin: false }]))
      : {},
  },
  build: {
    emptyOutDir: true,
    rollupOptions: {
      output: {
        entryFileNames: "assets/chat.js",
        assetFileNames: "assets/[name][extname]",
      },
    },
  },
});
