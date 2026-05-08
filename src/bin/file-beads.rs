//! Reads tests/conformance/beads.yaml and creates labels + issues in the
//! configured GitHub repo via `gh`. Idempotent: existing labels/issues with
//! the same name/title are skipped.
//!
//! Default: dry-run. Pass `--apply` to actually create.
//!
//!   cargo run --bin file-beads               # dry-run, prints plan
//!   cargo run --bin file-beads -- --apply    # creates labels + issues
//!   cargo run --bin file-beads -- --apply --beads F1,T1   # subset

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, serde::Deserialize)]
struct Beads {
    repo: String,
    labels: Vec<Label>,
    epics: Vec<Epic>,
    beads: Vec<Bead>,
}

#[derive(Debug, serde::Deserialize)]
struct Label {
    name: String,
    color: String,
    description: String,
}

#[derive(Debug, serde::Deserialize)]
struct Epic {
    id: String,
    title: String,
    labels: Vec<String>,
    body: String,
}

#[derive(Debug, serde::Deserialize)]
struct Bead {
    id: String,
    title: String,
    phase: serde_yaml::Value,
    area: String,
    files: Vec<String>,
    depends_on: Vec<String>,
    fixture: Option<String>,
    evidence: String,
    body: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut apply = false;
    let mut master = false;
    let mut subset: Option<BTreeSet<String>> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--apply" => apply = true,
            "--master" => master = true,
            "--beads" => {
                let csv = args.next().ok_or("--beads needs a CSV argument")?;
                subset = Some(csv.split(',').map(|s| s.trim().to_string()).collect());
            }
            other => return Err(format!("unknown arg: {}", other).into()),
        }
    }

    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = workspace.join("tests/conformance/beads.yaml");
    let body = fs::read_to_string(&path)?;
    let beads: Beads = serde_yaml::from_str(&body)?;

    println!(
        "repo: {} ({} labels, {} epics, {} beads)",
        beads.repo,
        beads.labels.len(),
        beads.epics.len(),
        beads.beads.len()
    );

    if !apply {
        println!("\n--- DRY RUN ---  (pass --apply to create)\n");
    }

    if master {
        let title = "OpenAPI 3.1 / 3.2 Conformance: master tracking issue".to_string();
        let body = compose_master_body(&beads);
        let labels = vec!["epic".to_string()];
        if apply {
            let _ = create_issue(&beads.repo, &title, &body, &labels)?;
        } else {
            println!("== master epic ==");
            println!("  title: {}", title);
            println!("  body bytes: {}", body.len());
            println!(
                "  preview (first 60 lines):\n----\n{}\n----",
                body.lines().take(60).collect::<Vec<_>>().join("\n")
            );
        }
        if !apply {
            println!("\n(dry-run complete — re-run with --apply --master to create on GitHub)");
        }
        return Ok(());
    }

    // Phase 1: labels
    println!("== labels ==");
    for label in &beads.labels {
        if apply {
            create_label(&beads.repo, label)?;
        } else {
            println!("  + label {} ({}, {})", label.name, label.color, label.description);
        }
    }

    // Phase 2: epics — file first so beads can reference them.
    println!("\n== epics ==");
    let mut epic_numbers: BTreeMap<String, u64> = BTreeMap::new();
    for epic in &beads.epics {
        let number = if apply {
            create_issue(&beads.repo, &epic.title, &epic.body, &epic.labels)?
        } else {
            println!(
                "  + epic [{}] {} ({} labels)",
                epic.id,
                epic.title,
                epic.labels.len()
            );
            0
        };
        epic_numbers.insert(epic.id.clone(), number);
    }

    // Phase 3: beads — each one labeled and linked back to its epic.
    println!("\n== beads ==");
    let phase_to_epic = phase_to_epic_map();
    let phase_to_label = phase_to_label_map();

    for bead in &beads.beads {
        if let Some(s) = &subset {
            if !s.contains(&bead.id) {
                continue;
            }
        }
        let phase_str = phase_string(&bead.phase);
        let epic_id = phase_to_epic.get(phase_str.as_str()).copied().unwrap_or("");
        let epic_number = epic_numbers.get(epic_id).copied().unwrap_or(0);

        let labels = bead_labels(bead, &phase_str, &phase_to_label);
        let body = compose_bead_body(bead, epic_id, epic_number);
        let title = format!("[{}] {}", bead.id, bead.title);

        if apply {
            let _ = create_issue(&beads.repo, &title, &body, &labels)?;
        } else {
            println!(
                "  + bead [{}] phase={} area={} files={:?} deps={:?}",
                bead.id, phase_str, bead.area, bead.files, bead.depends_on
            );
        }
    }

    if !apply {
        println!("\n(dry-run complete — re-run with --apply to create on GitHub)");
    }
    Ok(())
}

fn phase_string(v: &serde_yaml::Value) -> String {
    match v {
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::String(s) => s.clone(),
        _ => "?".to_string(),
    }
}

fn phase_to_epic_map() -> BTreeMap<&'static str, &'static str> {
    [("0", "E0"), ("1", "E1"), ("2", "E2"), ("2b", "E2"), ("3", "E3")]
        .into_iter()
        .collect()
}

fn phase_to_label_map() -> BTreeMap<&'static str, &'static str> {
    [
        ("0", "phase:0-foundation"),
        ("1", "phase:1-silent-fail"),
        ("2", "phase:2-3.1-honesty"),
        ("2b", "phase:2b-json-schema"),
        ("3", "phase:3-3.2-deltas"),
    ]
    .into_iter()
    .collect()
}

fn bead_labels(
    bead: &Bead,
    phase_str: &str,
    phase_to_label: &BTreeMap<&str, &str>,
) -> Vec<String> {
    let mut labels = vec!["bead".to_string()];
    if let Some(p) = phase_to_label.get(phase_str) {
        labels.push((*p).to_string());
    }
    labels.push(format!("area:{}", normalize_area(&bead.area)));
    if bead.fixture.is_some() {
        labels.push("conformance".to_string());
    }
    labels
}

fn normalize_area(area: &str) -> &str {
    // Map free-form area names in beads.yaml to label suffixes that exist in
    // the labels list. Anything else stays as-is.
    match area {
        "schema" | "paths" | "bodies" | "security" | "components" | "codegen" | "refs" => area,
        "webhooks" => "components",
        "tags" => "components",
        "foundation" => "schema",
        _ => area,
    }
}

fn compose_bead_body(bead: &Bead, epic_id: &str, epic_number: u64) -> String {
    let phase_str = phase_string(&bead.phase);
    let mut s = String::new();
    s.push_str(&format!("**Bead ID:** `{}`\n", bead.id));
    s.push_str(&format!("**Phase:** {}\n", phase_str));
    s.push_str(&format!("**Area:** {}\n", bead.area));
    if epic_number != 0 {
        s.push_str(&format!("**Epic:** #{} (`{}`)\n", epic_number, epic_id));
    } else {
        s.push_str(&format!("**Epic:** `{}`\n", epic_id));
    }
    if !bead.depends_on.is_empty() {
        s.push_str(&format!("**Depends on:** {}\n", bead.depends_on.join(", ")));
    }
    if let Some(fx) = &bead.fixture {
        s.push_str(&format!("**Fixture:** `{}`\n", fx));
    }
    s.push_str(&format!("**Files:** {}\n", bead.files.join(", ")));
    s.push_str(&format!("**Evidence:** {}\n\n", bead.evidence));
    s.push_str(bead.body.trim_end());
    s.push_str(
        "\n\n---\n\n*Bead managed via `tests/conformance/beads.yaml`. Edit there and re-run `cargo run --bin file-beads -- --apply`.*\n",
    );
    s
}

fn compose_master_body(b: &Beads) -> String {
    let mut s = String::new();
    s.push_str(
        "# OpenAPI 3.1 / 3.2 Conformance — Master Tracking Issue\n\n\
         This is the single source-of-truth issue for getting the generator to honestly support \
         OpenAPI 3.1 and 3.2. We will comment on this issue as work progresses rather than \
         filing 50+ child issues.\n\n",
    );

    s.push_str("## Background\n\n");
    s.push_str(
        "Six parallel audit agents reviewed the codebase against the OpenAPI 3.1.2 (2025-09-19) \
         and OpenAPI 3.2.0 (2025-09-19) specs. Reports live under \
         `tmp/openapi-specs/reports/` (and `00-SUMMARY.md` consolidates).\n\n\
         **Headline finding:** the README claims OpenAPI 3.1 support; in practice the generator \
         is OpenAPI 3.0-shaped with a thin 3.1 veneer. A 3.1- or 3.2-only spec parses without \
         error and silently drops most semantics. Architectural root cause: every parsing struct \
         ends with `#[serde(flatten)] extra: BTreeMap<String, Value>` and there is no \
         `deny_unknown_fields` anywhere.\n\n",
    );

    s.push_str("## Conformance harness\n\n");
    s.push_str(
        "The repo now contains a self-validating conformance harness:\n\n\
         - `tests/conformance/specs/` — committed copies of OAS 3.1.2 and 3.2.0 markdown.\n\
         - `tests/conformance/catalog.yaml` — generated from the 3.2.0 spec by \
         `cargo run --bin catalog-gen`. 30 OAS Objects, 141 fields, 57 JSON Schema 2020-12 \
         keywords, 7 parameter-style combos. This is the **denominator** for \"100% coverage\".\n\
         - `tests/conformance/fixtures/` — atomic fixtures, each tagged with `coverage:` \
         entries from the catalog and a `fails_at: L0|L1|L3|...` marker. The harness \
         (`tests/conformance.rs`) only enforces `ACTIVE_LAYERS`; later layers light up as \
         they are implemented.\n\
         - `tests/conformance/external/json-schema-test-suite/` — git submodule of the canonical \
         JSON Schema 2020-12 corpus. Run via `tests/conformance_json_schema.rs`.\n\
         - `tests/conformance/external/apis-guru-sync.sh` — lazy-clones the APIs.guru \
         openapi-directory for nightly real-world smoke (`APIS_GURU_SMOKE=1`).\n\
         - `tests/conformance/status.toml` — honest support claims, derived from harness \
         results. README will pull from this.\n\n",
    );

    s.push_str("## Beads — units of independently-mergeable work\n\n");
    s.push_str(&format!(
        "All {} beads + {} epics are described in `tests/conformance/beads.yaml`. The tables \
         below are a rendered view; edit beads.yaml as the source of truth.\n\n\
         **Parallelization rule:** two beads may run concurrently iff their `files` sets are \
         disjoint AND neither is in the other's `depends_on` closure.\n\n",
        b.beads.len(),
        b.epics.len()
    ));

    for phase in ["0", "1", "2", "2b", "3"] {
        let phase_beads: Vec<&Bead> = b
            .beads
            .iter()
            .filter(|x| phase_string(&x.phase) == phase)
            .collect();
        if phase_beads.is_empty() {
            continue;
        }
        s.push_str(&format!("\n### Phase {}\n\n", phase));
        s.push_str("| | ID | Title | Area | Files | Depends on |\n");
        s.push_str("|---|---|---|---|---|---|\n");
        for bd in phase_beads {
            s.push_str(&format!(
                "| [ ] | `{}` | {} | {} | `{}` | {} |\n",
                bd.id,
                escape_md(&bd.title),
                bd.area,
                bd.files.join(" "),
                if bd.depends_on.is_empty() {
                    "—".to_string()
                } else {
                    bd.depends_on.join(", ")
                },
            ));
        }
    }

    s.push_str("\n## Parallel-execution batches\n\n");
    s.push_str(&compose_parallel_plan(&b.beads));

    s.push_str("\n## How we'll work\n\n");
    s.push_str(
        "1. **Phase 0 lands first** as a single PR (or as F1, F2, F3, F4 in sequence). Without \
         `deny_unknown_fields`, every later fix is unverifiable.\n\
         2. **Each subsequent bead is a focused PR**, scoped to its `files` set, with a \
         conformance fixture flipping from `fails_at:` failing to passing.\n\
         3. **Progress is tracked in this issue's comments**: one comment per merged bead with \
         the bead ID, PR link, and which `status.toml` entries flipped.\n\
         4. **We don't stage 50 child issues** — the granularity lives in `beads.yaml` and PR \
         titles. This issue is the dashboard.\n\
         5. **Honesty gate:** `tests/conformance/status.toml` is regenerated by the harness; \
         the README's support claims will be derived from it. Drift fails CI.\n",
    );

    s.push_str("\n## References\n\n");
    s.push_str(
        "- Audit reports: `tmp/openapi-specs/reports/00-SUMMARY.md` and per-area files \
         (01-schema, 02-paths-parameters, 03-bodies-media, 04-servers-security, \
         05-components-refs-webhooks, 06-three-two-deltas).\n\
         - Spec: `tests/conformance/specs/openapi-3.2.0.md`, `openapi-3.1.2.md`.\n\
         - Catalog: `tests/conformance/catalog.yaml`.\n\
         - Beads source-of-truth: `tests/conformance/beads.yaml`.\n",
    );

    s
}

fn compose_parallel_plan(beads: &[Bead]) -> String {
    // Compute the longest dependency chain depth for each bead. Beads at the
    // same depth with disjoint file sets can run concurrently.
    let depth = compute_depths(beads);
    let mut by_depth: BTreeMap<usize, Vec<&Bead>> = BTreeMap::new();
    for b in beads {
        by_depth.entry(depth[&b.id]).or_default().push(b);
    }

    let mut s = String::new();
    s.push_str(
        "Beads grouped by dependency depth. Within a depth tier, beads with **disjoint file sets** \
         can be worked in parallel; beads sharing files must serialize.\n\n",
    );
    for (d, group) in &by_depth {
        s.push_str(&format!("### Tier {}\n\n", d));
        let parallel = max_parallel_groups(group);
        for (i, batch) in parallel.iter().enumerate() {
            let ids: Vec<String> = batch.iter().map(|b| b.id.clone()).collect();
            let files: BTreeSet<String> = batch
                .iter()
                .flat_map(|b| b.files.iter().cloned())
                .collect();
            s.push_str(&format!(
                "- Batch {}.{}: {} (touches {})\n",
                d,
                i + 1,
                ids.join(", "),
                files.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }
        s.push('\n');
    }
    s
}

fn compute_depths(beads: &[Bead]) -> BTreeMap<String, usize> {
    let by_id: BTreeMap<&str, &Bead> = beads.iter().map(|b| (b.id.as_str(), b)).collect();
    let mut depth = BTreeMap::new();
    fn dfs(
        id: &str,
        by_id: &BTreeMap<&str, &Bead>,
        depth: &mut BTreeMap<String, usize>,
    ) -> usize {
        if let Some(d) = depth.get(id) {
            return *d;
        }
        let bead = match by_id.get(id) {
            Some(b) => b,
            None => return 0,
        };
        let d = if bead.depends_on.is_empty() {
            0
        } else {
            bead.depends_on
                .iter()
                .map(|dep| 1 + dfs(dep, by_id, depth))
                .max()
                .unwrap_or(0)
        };
        depth.insert(id.to_string(), d);
        d
    }
    for b in beads {
        dfs(&b.id, &by_id, &mut depth);
    }
    depth
}

/// Greedy partition: each batch is a set of beads with mutually-disjoint file
/// touches. Approximates the minimum number of serial waves at this tier.
fn max_parallel_groups<'a>(beads: &[&'a Bead]) -> Vec<Vec<&'a Bead>> {
    let mut batches: Vec<(BTreeSet<String>, Vec<&'a Bead>)> = Vec::new();
    for b in beads {
        let mine: BTreeSet<String> = b.files.iter().cloned().collect();
        let mut placed = false;
        for (used, batch) in &mut batches {
            if used.intersection(&mine).next().is_none() {
                used.extend(mine.iter().cloned());
                batch.push(b);
                placed = true;
                break;
            }
        }
        if !placed {
            batches.push((mine, vec![b]));
        }
    }
    batches.into_iter().map(|(_, b)| b).collect()
}

fn escape_md(s: &str) -> String {
    s.replace('|', "\\|")
}

fn create_label(repo: &str, label: &Label) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("gh")
        .args([
            "label", "create", &label.name,
            "--repo", repo,
            "--color", &label.color,
            "--description", &label.description,
            "--force",
        ])
        .status()?;
    if !status.success() {
        eprintln!("warning: label create failed for {} (may already exist)", label.name);
    }
    Ok(())
}

fn create_issue(
    repo: &str,
    title: &str,
    body: &str,
    labels: &[String],
) -> Result<u64, Box<dyn std::error::Error>> {
    // Dedup: skip if an open issue with the same title already exists.
    let existing = Command::new("gh")
        .args(["issue", "list", "--repo", repo, "--state", "all", "--search", title, "--json", "number,title", "--limit", "20"])
        .output()?;
    let existing_json: serde_json::Value = serde_json::from_slice(&existing.stdout).unwrap_or(serde_json::Value::Array(vec![]));
    if let Some(arr) = existing_json.as_array() {
        for v in arr {
            if v.get("title").and_then(|t| t.as_str()) == Some(title) {
                let n = v.get("number").and_then(|n| n.as_u64()).unwrap_or(0);
                println!("  = #{} (already exists) {}", n, title);
                return Ok(n);
            }
        }
    }

    let mut args = vec![
        "issue".to_string(),
        "create".to_string(),
        "--repo".to_string(), repo.to_string(),
        "--title".to_string(), title.to_string(),
        "--body".to_string(), body.to_string(),
    ];
    for l in labels {
        args.push("--label".to_string());
        args.push(l.clone());
    }
    let output = Command::new("gh").args(&args).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gh issue create failed: {}", stderr).into());
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let number = url.rsplit('/').next().and_then(|s| s.parse().ok()).unwrap_or(0);
    println!("  + #{} {}", number, title);
    Ok(number)
}
