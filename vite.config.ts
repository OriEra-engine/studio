import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  plugins: [vue()],
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
