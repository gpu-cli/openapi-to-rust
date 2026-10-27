// Run after Astro builds: every problem type emitted by the generator must
// resolve to its own documentation page, including transport-only helpers.
import { readFile } from "node:fs/promises";

const source = await readFile(new URL("../../src/server/validation.rs", import.meta.url), "utf8");
const uris = new Set(source.match(/https:\/\/openapi-to-rust\.dev\/problems\/[a-z-]+/g));
if (uris.size === 0) throw new Error("No generated problem type URIs found; check the audit's source path/pattern.");

for (const uri of uris) {
  const { pathname } = new URL(uri);
  const html = await readFile(new URL(`../dist${pathname}/index.html`, import.meta.url), "utf8");
  if (!html.includes(`<link rel="canonical" href="${uri}"`) || !html.includes(uri)) {
    throw new Error(`${uri}: missing canonical problem documentation`);
  }
}
console.log(`Problem documentation audit passed for ${uris.size} generated type URIs.`);
