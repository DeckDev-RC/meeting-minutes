import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  build: {
    modulePreload: false,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes("vite/preload-helper")) {
            return "vite-preload-helper";
          }

          if (!id.includes("node_modules")) return undefined;

          if (/[\\/]node_modules[\\/](react|react-dom|react-router-dom)[\\/]/.test(id)) {
            return "vendor-react";
          }

          if (id.includes("@tauri-apps")) {
            return "vendor-tauri";
          }

          if (id.includes("html2canvas")) {
            return "vendor-canvas";
          }

          if (id.includes("jspdf") || id.includes("fflate")) {
            return "vendor-jspdf";
          }

          if (id.includes("dompurify")) {
            return "vendor-sanitize";
          }

          if (id.includes("zustand")) {
            return "vendor-state";
          }

          return undefined;
        },
      },
    },
  },
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});
