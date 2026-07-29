import { createHash } from "node:crypto";
import { basename } from "node:path";
import { readFile, writeFile } from "node:fs/promises";

const [archivePath] = process.argv.slice(2);
if (!archivePath) {
  throw new Error("usage: write-checksum.mjs <archive>");
}

const checksum = createHash("sha256").update(await readFile(archivePath)).digest("hex");
await writeFile(`${archivePath}.sha256`, `${checksum}  ${basename(archivePath)}\n`);
