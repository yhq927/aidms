import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";

// Tauri 忽略约定：把环境变量前缀改为自定义，避免与 Tauri 冲突
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  // Tauri 下固定端口，避免 HMR 端口漂移
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: false,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "es2021",
    outDir: "dist",
  },
});
