import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);

// Tauri 忽略约定：把环境变量前缀改为自定义，避免与 Tauri 冲突
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  // 显式指定 PostCSS + lightningcss 压缩器
  // PostCSS：绕开 postcss.config.js 的 ESM 解析问题（Vite 在 "type":"module" 项目中
  //         无法自动加载 .js 版配置，导致 Tailwind 从未被执行）
  // lightningcss：替代 esbuild 做压缩；esbuild 会错误丢弃全部 Tailwind @layer 输出
  css: {
    postcss: {
      plugins: [
        require("tailwindcss"),
        require("autoprefixer"),
      ],
    },
    lightningcss: {
      targets: { chrome: 120, edge: 120, firefox: 120, safari: 16 },
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
    // 关键：用 lightningcss 替代 esbuild 做 CSS 压缩。
    // esbuild 的 CSS minifier 会错误丢弃全部 Tailwind @layer 输出，
    // 导致生产构建产物 CSS 只剩 driver.js 样式、页面裸 HTML 无样式。
    cssMinify: "lightningcss",
  },
});
