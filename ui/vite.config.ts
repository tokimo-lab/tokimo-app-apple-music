import { resolve } from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Library mode with externals: React, ReactDOM, the Tokimo UI library and
// the SDK are provided by the shell at runtime via an import map → shim
// modules that re-export window.__TKM_DEPS__. This keeps a SINGLE React
// instance across the shell and every app (mandatory for hooks).
//
// Bundles every other dep (lucide-react, app code, CSS) into one ESM file.
const EXTERNAL = [
  "react",
  "react/jsx-runtime",
  "react-dom",
  "react-dom/client",
  "@tokimo/ui",
  "@tokimo/sdk",
];

export default defineConfig({
  plugins: [react(), tailwindcss()],
  // react-query (and other libs) reference process.env.NODE_ENV at runtime;
  // statically replace so the bundle works in the host browser without a
  // `process` global.
  define: {
    "process.env.NODE_ENV": JSON.stringify("production"),
    "process.env": JSON.stringify({}),
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    cssCodeSplit: false,
    sourcemap: false,
    minify: "esbuild",
    target: "es2022",
    lib: {
      entry: resolve(__dirname, "src/index.tsx"),
      formats: ["es"],
      fileName: () => "index.js",
      cssFileName: "index",
    },
    rollupOptions: {
      external: (id) => EXTERNAL.includes(id),
      output: {
        inlineDynamicImports: true,
      },
    },
  },
});
