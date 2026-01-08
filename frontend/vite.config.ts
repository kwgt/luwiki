import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';
import path from 'node:path';

export default defineConfig({
  plugins: [vue()],
  base: '/static/',
  publicDir: 'public',
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: {
      input: {
        index: path.resolve(__dirname, 'index.html'),
      },
      output: {
        entryFileNames: (chunk) =>
					(chunk.name === 'index' ? 'app.[hash].js' : '[name].[hash].js'),

        chunkFileNames: 'chunk-[name].[hash].js',
        assetFileNames: (assetInfo) => {
          if (assetInfo.name && assetInfo.name.endsWith('.css')) {
            return 'app.[hash].css';
          }
          return 'assets/[name].[hash][extname]';
        },
      },
    },
  },
});
