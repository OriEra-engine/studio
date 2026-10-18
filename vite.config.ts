import { readFileSync } from "node:fs";
import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

const pkg = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf-8"));

export default defineConfig({
  plugins: [vue()],
  // 单一版本源：package.json。侧栏/关于页引用 __APP_VERSION__，
  // 杜绝「升级了却还写着 v0.1」这类手滑（2026-10-18 审计发现）。
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    target: "chrome105",
    // WebView2 自定义协议(tauri.localhost)下动态 import 的懒加载 chunk 会静默失败，
    // 表现为主界面空白——这里关闭代码分割打成单 bundle 规避
    rollupOptions: {
      output: {
        inlineDynamicImports: true,
      },
    },
  },
});
