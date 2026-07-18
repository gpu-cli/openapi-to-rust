// Fetch the playground WASM bundle pinned by playground-wasm.lock.
//
// Runs as npm predev/prebuild. No-op when public/playground/pkg/ already
// exists (a local scripts/build-playground-wasm.sh build wins); otherwise
// downloads the pinned release asset so Vercel builds and toolchain-less
// clones get a working playground. Fails loudly rather than building a
// playground page with no wasm behind it.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const websiteDir = dirname(dirname(fileURLToPath(import.meta.url)));
const pkgDir = join(websiteDir, "public", "playground", "pkg");
const lockPath = join(websiteDir, "playground-wasm.lock");

if (existsSync(join(pkgDir, "openapi_to_rust_wasm_bg.wasm"))) {
  console.log("playground wasm: using existing local build in public/playground/pkg/");
  process.exit(0);
}

const tag = readFileSync(lockPath, "utf8").trim();
if (!tag) {
  console.error(`playground wasm: ${lockPath} is empty`);
  process.exit(1);
}

const url = `https://github.com/gpu-cli/openapi-to-rust/releases/download/${tag}/playground-pkg.tar.gz`;
console.log(`playground wasm: fetching ${url}`);

const response = await fetch(url, { redirect: "follow" });
if (!response.ok) {
  console.error(
    `playground wasm: download failed with HTTP ${response.status}. ` +
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

if (!existsSync(join(pkgDir, "openapi_to_rust_wasm_bg.wasm"))) {
  console.error("playground wasm: archive did not contain pkg/openapi_to_rust_wasm_bg.wasm");
  process.exit(1);
}
console.log(`playground wasm: staged ${tag} into public/playground/pkg/`);
