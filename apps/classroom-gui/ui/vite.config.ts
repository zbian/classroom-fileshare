import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { fileURLToPath, URL } from 'node:url'

// 三入口：icon 浮动图标 / main 主界面 / tips 气泡（lead 2026-11-13 修订采纳）
// 对齐 tauri.conf.json 的 build.devUrl（http://localhost:1420）与 frontendDist（../ui/dist）
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  envPrefix: 'VITE_',
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    // WebView2 Evergreen 内核，无需兼容旧浏览器
    target: 'chrome110',
    rollupOptions: {
      input: {
        icon: fileURLToPath(new URL('./icon.html', import.meta.url)),
        main: fileURLToPath(new URL('./main.html', import.meta.url)),
        tips: fileURLToPath(new URL('./tips.html', import.meta.url)),
      },
    },
  },
})
