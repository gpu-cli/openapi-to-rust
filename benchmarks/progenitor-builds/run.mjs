#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "../..");
const options = parseArgs(process.argv.slice(2));
const generator = path.resolve(options.generator ?? path.join(repoRoot, "target/release/openapi-to-rust"));

if (!fs.existsSync(generator)) {
  fail(`generator binary not found: ${generator}\nRun: cargo build --release --bin openapi-to-rust`);
}

const benchRoot = fs.mkdtempSync(path.join(os.tmpdir(), "openapi-progenitor-builds-"));
const aotRoot = path.join(benchRoot, "aot");
const macroRoot = path.join(benchRoot, "macro");
const results = {
  schemaVersion: 1,
  recordedAt: new Date().toISOString(),
  fixture: {
    kind: "synthetic-openapi-3.0.3",
    operations: options.operations,
    schemas: options.operations,
  },
  versions: {
    openapiToRust: capture(generator, ["--version"]),
    progenitor: "0.14.0",
    rustc: capture("rustc", ["--version"]),
    cargo: capture("cargo", ["--version"]),
    node: process.version,
  },
  machine: {
    platform: os.platform(),
    release: os.release(),
    arch: os.arch(),
    cpu: os.cpus()[0]?.model ?? "unknown",
    logicalCpus: os.cpus().length,
    memoryBytes: os.totalmem(),
  },
  methodology: {
    samples: options.samples,
    cargoNetworkMode: "offline after lockfile resolution",
    cleanBuild: "unique Cargo target and build directories per sample",
    noOp: "unchanged cargo check using a warmed target",
    rustTouch: "touch src/lib.rs, then cargo check",
    specChangeAot: "change schema description, run release generator, then cargo check",
    specChangeMacro: "change schema description, then cargo check",
  },
  samples: [],
};

try {
  prepareCrates();
  resolveLockfiles();
  measureCleanBuilds();
  measureIncrementalBuilds();
  results.summary = summarize(results.samples);

  const rendered = `${JSON.stringify(results, null, 2)}\n`;
  if (options.output) {
    const output = path.resolve(options.output);
    fs.mkdirSync(path.dirname(output), { recursive: true });
    fs.writeFileSync(output, rendered);
    process.stderr.write(`wrote ${output}\n`);
  } else {
    process.stdout.write(rendered);
  }
} finally {
  if (options.keep) {
    process.stderr.write(`kept benchmark workspace: ${benchRoot}\n`);
  } else {
    fs.rmSync(benchRoot, { recursive: true, force: true });
  }
}

function parseArgs(args) {
  const parsed = { samples: 5, operations: 120, keep: false };
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === "--generator") parsed.generator = requireValue(args, ++index, argument);
    else if (argument === "--output") parsed.output = requireValue(args, ++index, argument);
    else if (argument === "--samples") parsed.samples = positiveInteger(requireValue(args, ++index, argument), argument);
    else if (argument === "--operations") parsed.operations = positiveInteger(requireValue(args, ++index, argument), argument);
    else if (argument === "--keep") parsed.keep = true;
    else if (argument === "--help") {
      process.stdout.write("Usage: node benchmarks/progenitor-builds/run.mjs [--generator PATH] [--samples N] [--operations N] [--output PATH] [--keep]\n");
      process.exit(0);
    } else fail(`unknown argument: ${argument}`);
  }
  return parsed;
}

function requireValue(args, index, flag) {
  if (!args[index]) fail(`${flag} requires a value`);
  return args[index];
}

function positiveInteger(value, flag) {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isInteger(parsed) || parsed < 1) fail(`${flag} must be a positive integer`);
  return parsed;
}

function prepareCrates() {
  fs.mkdirSync(path.join(aotRoot, "src/generated"), { recursive: true });
  fs.mkdirSync(path.join(macroRoot, "src"), { recursive: true });
  const spec = `${JSON.stringify(makeFixture(options.operations), null, 2)}\n`;
  fs.writeFileSync(path.join(aotRoot, "openapi.json"), spec);
  fs.writeFileSync(path.join(macroRoot, "openapi.json"), spec);

  fs.writeFileSync(path.join(aotRoot, "Cargo.toml"), `[package]
name = "bench-openapi-aot"
version = "0.1.0"
edition = "2024"

[dependencies]
reqwest = { version = "=0.13.4", default-features = false, features = ["rustls"] }
reqwest-middleware = "=0.5.2"
serde = { version = "=1.0.229", features = ["derive"] }
serde_json = "=1.0.151"
thiserror = "=2.0.19"
`);
  fs.writeFileSync(path.join(aotRoot, "src/lib.rs"), `pub mod generated;

pub fn generated_type_name() -> &'static str {
    std::any::type_name::<generated::Item000>()
}
`);

  fs.writeFileSync(path.join(macroRoot, "Cargo.toml"), `[package]
name = "bench-progenitor-macro"
version = "0.1.0"
edition = "2024"

[dependencies]
progenitor = "=0.14.0"
reqwest = { version = "=0.13.4", default-features = false, features = ["json", "query", "stream"] }
serde = { version = "=1.0.229", features = ["derive"] }
`);
  fs.writeFileSync(path.join(macroRoot, "src/lib.rs"), `progenitor::generate_api!("openapi.json");

pub fn generated_type_name() -> &'static str {
    std::any::type_name::<types::Item000>()
}
`);

  run(generator, ["generate", "openapi.json", "--output-dir", "src/generated", "--quiet"], { cwd: aotRoot });
}

function resolveLockfiles() {
  run("cargo", ["generate-lockfile"], { cwd: aotRoot });
  run("cargo", ["generate-lockfile"], { cwd: macroRoot });
  run("cargo", ["fetch", "--locked"], { cwd: aotRoot });
  run("cargo", ["fetch", "--locked"], { cwd: macroRoot });
}

function measureCleanBuilds() {
  for (let sample = 0; sample < options.samples; sample += 1) {
    const order = sample % 2 === 0 ? ["aot", "macro"] : ["macro", "aot"];
    for (const variant of order) {
      const root = variant === "aot" ? aotRoot : macroRoot;
      const runRoot = path.join(benchRoot, "clean", `${sample}-${variant}`);
      measure("clean", variant, sample, () => cargoCheck(root, runRoot));
    }
  }
}

function measureIncrementalBuilds() {
  const aotRun = path.join(benchRoot, "incremental/aot");
  const macroRun = path.join(benchRoot, "incremental/macro");
  cargoCheck(aotRoot, aotRun);
  cargoCheck(macroRoot, macroRun);

  for (let sample = 0; sample < options.samples; sample += 1) {
    measure("noop", "aot", sample, () => cargoCheck(aotRoot, aotRun));
    measure("noop", "macro", sample, () => cargoCheck(macroRoot, macroRun));
  }

  measureTouchedScenario("rust-touch", path.join(aotRoot, "src/lib.rs"), path.join(macroRoot, "src/lib.rs"), aotRun, macroRun);
  measureSpecChanges(aotRun, macroRun);
}

function measureTouchedScenario(scenario, aotFile, macroFile, aotRun, macroRun) {
  for (let sample = 0; sample < options.samples; sample += 1) {
    const order = sample % 2 === 0 ? ["aot", "macro"] : ["macro", "aot"];
    for (const variant of order) {
      touch(variant === "aot" ? aotFile : macroFile, sample);
      measure(scenario, variant, sample, () => cargoCheck(variant === "aot" ? aotRoot : macroRoot, variant === "aot" ? aotRun : macroRun));
    }
  }
}

function measureSpecChanges(aotRun, macroRun) {
  for (let sample = 0; sample < options.samples; sample += 1) {
    const order = sample % 2 === 0 ? ["aot", "macro"] : ["macro", "aot"];
    for (const variant of order) {
      const root = variant === "aot" ? aotRoot : macroRoot;
      mutateSpec(path.join(root, "openapi.json"), sample);
      measure("spec-change", variant, sample, () => {
        if (variant === "aot") {
          run(generator, ["generate", "openapi.json", "--output-dir", "src/generated", "--quiet"], { cwd: aotRoot });
        }
        cargoCheck(root, variant === "aot" ? aotRun : macroRun);
      });
    }
  }
}

function cargoCheck(root, runRoot) {
  run("cargo", [
    "--offline",
    "--config", `build.build-dir="${path.join(runRoot, "build")}"`,
    "check", "--quiet", "--locked",
    "--manifest-path", path.join(root, "Cargo.toml"),
    "--target-dir", path.join(runRoot, "target"),
  ]);
}

function measure(scenario, variant, sample, action) {
  const started = process.hrtime.bigint();
  action();
  const durationSeconds = Number(process.hrtime.bigint() - started) / 1e9;
  results.samples.push({ scenario, variant, sample, durationSeconds });
  process.stderr.write(`${scenario} ${variant} ${sample}: ${durationSeconds.toFixed(3)}s\n`);
}

function summarize(samples) {
  const grouped = {};
  for (const sample of samples) {
    const key = `${sample.scenario}:${sample.variant}`;
    (grouped[key] ??= []).push(sample.durationSeconds);
  }
  return Object.fromEntries(Object.entries(grouped).map(([key, values]) => [key, {
    medianSeconds: median(values),
    minimumSeconds: Math.min(...values),
    maximumSeconds: Math.max(...values),
    samples: values.length,
  }]));
}

function median(values) {
  const sorted = [...values].sort((left, right) => left - right);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0 ? (sorted[middle - 1] + sorted[middle]) / 2 : sorted[middle];
}

function makeFixture(count) {
  const schemas = {};
  const paths = {};
  for (let index = 0; index < count; index += 1) {
    const suffix = String(index).padStart(3, "0");
    const schemaName = `Item${suffix}`;
    schemas[schemaName] = {
      type: "object",
      required: ["id", "name", "count"],
      properties: {
        id: { type: "string" },
        name: { type: "string" },
        count: { type: "integer", format: "int64" },
        enabled: { type: "boolean" },
        labels: { type: "array", items: { type: "string" } },
      },
    };
    paths[`/items/${suffix}`] = {
      get: {
        operationId: `getItem${suffix}`,
        parameters: [{ name: "verbose", in: "query", required: false, schema: { type: "boolean" } }],
        responses: {
          "200": {
            description: "Success",
            content: { "application/json": { schema: { $ref: `#/components/schemas/${schemaName}` } } },
          },
          "404": { description: "Not found" },
        },
      },
    };
  }
  return {
    openapi: "3.0.3",
    info: { title: "Synthetic build benchmark API", version: "1.0.0" },
    paths,
    components: { schemas },
  };
}

function touch(file, offset) {
  const timestamp = new Date(Date.now() + offset + 1);
  fs.utimesSync(file, timestamp, timestamp);
}

function mutateSpec(file, revision) {
  const document = JSON.parse(fs.readFileSync(file, "utf8"));
  document.components.schemas.Item000.description = `Benchmark revision ${revision}`;
  fs.writeFileSync(file, `${JSON.stringify(document, null, 2)}\n`);
}

function capture(command, args) {
  const completed = spawnSync(command, args, { encoding: "utf8" });
  if (completed.status !== 0) fail(`${command} ${args.join(" ")} failed: ${completed.stderr}`);
  return completed.stdout.trim();
}

function run(command, args, settings = {}) {
  const completed = spawnSync(command, args, { ...settings, encoding: "utf8" });
  if (completed.status !== 0) {
    fail(`${command} ${args.join(" ")} failed\n${completed.stdout ?? ""}${completed.stderr ?? ""}`);
  }
}

function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exit(1);
}
