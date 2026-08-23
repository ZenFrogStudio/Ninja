import { defineConfig } from 'tsup';

export default defineConfig([
  // Package build, for consumers that resolve dependencies themselves
  // (e.g. the settings UI).
  {
    entry: ['src/index.ts'],
    format: ['esm'],
    dts: true,
    outDir: 'dist',
  },
  // Self-contained browser build, served to widgets at
  // `/__ninja/client.js`. Widgets load this as a plain module with no
  // bundler and no import map, so every dependency has to be inlined.
  {
    entry: { client: 'src/index.ts' },
    format: ['esm'],
    dts: false,
    outDir: 'dist-browser',
    platform: 'browser',
    noExternal: [/.*/],
  },
]);
