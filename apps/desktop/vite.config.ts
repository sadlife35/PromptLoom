import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "path";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Exclude the Rust build output from Vite's file watcher.
      // On Windows, Node.js cannot watch .exe files while they are
      // being compiled/linked, causing an EBUSY crash.
      ignored: ["**/src-tauri/target/**"],
    },
  },
  build: {
    target: "es2021",
    outDir: "dist",
    rollupOptions: {
      input: {
        settings: resolve(__dirname, "index.html"),
        hud: resolve(__dirname, "hud.html"),
      },
    },
  },
});
