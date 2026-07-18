// Fetch the playground WASM bundle pinned by playground-wasm.lock.
//
// Runs as npm predev/prebuild. The staged bundle carries a .source-tag stamp:
//   - "local"            → built by scripts/build-playground-wasm.sh; always wins.
//   - matching lock tag  → already up to date; no-op.
//   - missing/mismatched → the lock moved on; refetch the pinned release asset.
// This keeps Vercel builds, toolchain-less clones, AND long-lived local
// checkouts on the lock's bundle without anyone remembering to update it.
// Fails loudly rather than building a playground page with no wasm behind it,
// but a refetch failure with a working bundle already staged only warns.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const websiteDir = dirname(dirname(fileURLToPath(import.meta.url)));
const pkgDir = join(websiteDir, "public", "playground", "pkg");
const wasmPath = join(pkgDir, "openapi_to_rust_wasm_bg.wasm");
const stampPath = join(pkgDir, ".source-tag");
const lockPath = join(websiteDir, "playground-wasm.lock");

const tag = readFileSync(lockPath, "utf8").trim();
if (!tag) {
  console.error(`playground wasm: ${lockPath} is empty`);
  process.exit(1);
}

const staged = existsSync(wasmPath);
const stamp = existsSync(stampPath) ? readFileSync(stampPath, "utf8").trim() : null;

if (staged && stamp === "local") {
  console.log("playground wasm: using local build in public/playground/pkg/ (stamped local)");
  process.exit(0);
}
if (staged && stamp === tag) {
  console.log(`playground wasm: ${tag} already staged`);
  process.exit(0);
}
if (staged) {
  console.log(
    `playground wasm: staged bundle is ${stamp ?? "unstamped"}, lock wants ${tag} — refetching`,
  );
}

const url = `https://github.com/gpu-cli/openapi-to-rust/releases/download/${tag}/playground-pkg.tar.gz`;
console.log(`playground wasm: fetching ${url}`);

let response;
try {
  response = await fetch(url, { redirect: "follow" });
} catch (error) {
  response = { ok: false, status: `network error: ${error instanceof Error ? error.message : error}` };
}
if (!response.ok) {
  if (staged) {
    console.warn(
      `playground wasm: refetch failed (${response.status}); keeping the staged ${stamp ?? "unstamped"} bundle`,
    );
    process.exit(0);
  }
  console.error(
    `playground wasm: download failed with ${response.status}. ` +
      `The release asset for ${tag} is missing — check the playground-wasm workflow.`,
  );
  process.exit(1);
}

const archivePath = join(websiteDir, "playground-pkg.tar.gz");
writeFileSync(archivePath, Buffer.from(await response.arrayBuffer()));
rmSync(pkgDir, { recursive: true, force: true });
mkdirSync(dirname(pkgDir), { recursive: true });
execFileSync("tar", ["xzf", archivePath, "-C", dirname(pkgDir)], { stdio: "inherit" });
rmSync(archivePath);

if (!existsSync(wasmPath)) {
  console.error("playground wasm: archive did not contain pkg/openapi_to_rust_wasm_bg.wasm");
  process.exit(1);
}
writeFileSync(stampPath, `${tag}\n`);
console.log(`playground wasm: staged ${tag} into public/playground/pkg/`);
