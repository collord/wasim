import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";

// The Rust engine's wasm-pack output lives outside this package at ../engine/pkg
// (built via ../engine/build-wasm.sh, gitignored). Allow the dev server to serve it
// (notably wasim_engine_bg.wasm, which the --target web glue fetches at runtime).
const enginePkg = fileURLToPath(new URL("../engine/pkg", import.meta.url));

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5180,
    open: true,
    fs: { allow: [".", enginePkg] },
  },
});
