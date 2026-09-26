import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: {
      // 忽略 Rust 编译产物，避免 cargo 编译时 Windows 文件锁导致 EBUSY 崩溃
      ignored: ["**/src-tauri/target/**", "**/src-tauri/gen/**"],
    },
  },
  build: {
    target: "chrome105",
    outDir: "dist",
  },
});
