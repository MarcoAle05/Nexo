import { defineConfig } from 'vite';

// Configuración recomendada por Tauri: puerto fijo y sin limpiar la consola.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: {
    target: 'es2022',
    outDir: 'dist',
  },
});
