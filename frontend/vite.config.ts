import path from 'node:path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import wasm from 'vite-plugin-wasm'
import topLevelAwait from 'vite-plugin-top-level-await'

export default defineConfig({
  plugins: [react(), wasm(), topLevelAwait()],
  resolve: {
    alias: {
      // Points at the wasm-pack output; build with: cd ../engine && ./build-wasm.sh
      '@engine': path.resolve(__dirname, '../engine/pkg'),
    },
  },
  worker: {
    // Module workers let the sim worker import WASM ESM directly
    format: 'es',
    plugins: () => [wasm(), topLevelAwait()],
  },
  optimizeDeps: {
    // Don't pre-bundle the WASM package — Vite's WASM plugin handles it
    exclude: ['@engine'],
  },
  server: {
    fs: {
      // Allow serving files from outside the frontend root (engine/pkg)
      allow: ['..'],
    },
  },
})
