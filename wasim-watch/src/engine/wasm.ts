// Loader for the Rust WaSim engine compiled to WASM (../engine/pkg, wasm-pack --target web).
// Rebuild with ../engine/build-wasm.sh after changing the Rust engine.
//
// The --target web glue exports a default init(url?) that instantiates the module (fetching
// wasim_engine_bg.wasm), plus the wasm_bindgen classes. We init once and hand back a WasmEngine
// bound to a model. WasmEngine exposes run_json (batch) and start() -> RunHandle (streaming).
import init, { WasmEngine, type RunHandle } from "../../../engine/pkg/wasim_engine.js";

export type { RunHandle };
export { WasmEngine };

let initialized: Promise<void> | null = null;

/** Instantiate the wasm module exactly once (idempotent across callers). */
export function ensureWasm(): Promise<void> {
  if (!initialized) initialized = init().then(() => undefined);
  return initialized;
}

/** Load a v2 model JSON into a fresh engine instance (module initialized on first call). */
export async function loadEngine(modelJson: string): Promise<WasmEngine> {
  await ensureWasm();
  return new WasmEngine(modelJson);
}
