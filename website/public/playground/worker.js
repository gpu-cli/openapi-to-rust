// Playground generation worker. Plain JS on purpose: this file is served
// verbatim from public/ and imports the wasm-pack bundle at runtime, so the
// site bundler never needs to understand the wasm module graph.
import init, { generate, version } from "/playground/pkg/openapi_to_rust_wasm.js";

const ready = init().then(() => {
  postMessage({ type: "ready", version: version() });
});

addEventListener("message", async (event) => {
  const { id, spec, sourceLabel } = event.data;
  try {
    await ready;
    const result = generate(spec, sourceLabel);
    postMessage({ type: "result", id, ok: true, result });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    postMessage({ type: "result", id, ok: false, error: message });
  }
});
