import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import AutoImport from 'unplugin-auto-import/vite'
import Components from 'unplugin-vue-components/vite'
import { ElementPlusResolver } from 'unplugin-vue-components/resolvers'
import { fileURLToPath, URL } from 'node:url'

export default defineConfig({
  plugins: [
    vue(),
    AutoImport({ resolvers: [ElementPlusResolver()] }),
    Components({ resolvers: [ElementPlusResolver()] }),
  ],
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  server: {
    proxy: {
      // ws:true is required for /api/realtime/ws and /api/files/terminal/ws.
      '/api': { target: 'http://localhost:7200', changeOrigin: true, ws: true },
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    chunkSizeWarningLimit: 900,
    rollupOptions: {
      output: {
        // Split the heavy vendors so a panel update does not re-download them,
        // and so routes that never chart (files, sites) skip echarts entirely.
        manualChunks: {
          echarts: ['echarts', 'echarts/core', 'echarts/charts', 'echarts/components', 'echarts/renderers'],
          'element-plus': ['element-plus', '@element-plus/icons-vue'],
          vue: ['vue', 'vue-router', 'pinia'],
          xterm: ['xterm', '@xterm/addon-fit'],
        },
      },
    },
  },
})