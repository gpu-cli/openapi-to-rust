import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";

const [archivePath, checksumPath] = process.argv.slice(2);
if (!archivePath || !checksumPath) {
  throw new Error("usage: verify-download.mjs <archive> <checksum>");
}

const expected = (await readFile(checksumPath, "utf8")).trim().split(/\s+/)[0];
const actual = createHash("sha256").update(await readFile(archivePath)).digest("hex");
if (actual !== expected) {
  throw new Error(`checksum mismatch for ${archivePath}: expected ${expected}, got ${actual}`);
}
