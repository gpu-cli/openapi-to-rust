import { appendFile, readFile } from "node:fs/promises";

const [reportPath, exitCode = "1"] = process.argv.slice(2);
let report;
let parseError;
try {
  report = JSON.parse(await readFile(reportPath, "utf8"));
} catch (error) {
  parseError = String(error);
  report = { clients: [] };
}

const clients = Array.isArray(report.clients) ? report.clients : [];
const changed = clients.filter((client) => client.spec_changed || client.output_changed).length;
const failed = clients.filter((client) => client.error).length + (parseError ? 1 : 0);
await appendFile(
  process.env.GITHUB_OUTPUT,
  `changed=${changed}\nfailed=${failed}\nreport=${reportPath}\nexit_code=${exitCode}\n`,
);

if (process.env.GITHUB_STEP_SUMMARY) {
  const rows = clients.map((client) => {
    const status = client.error ? "Failed" : "Passed";
    const drift = client.spec_changed ? "Updated" : "Unchanged";
    const detail = String(client.error ?? "Generated and compiled")
      .replaceAll("|", "\\|")
      .replaceAll("\n", "<br>");
    return `| ${client.name} | ${status} | ${drift} | ${detail} |`;
  });
  if (parseError) {
    const detail = parseError.replaceAll("|", "\\|").replaceAll("\n", "<br>");
    rows.push(`| workflow | Failed | Unchanged | Could not read the client report: ${detail} |`);
  }
  await appendFile(
    process.env.GITHUB_STEP_SUMMARY,
    [
      "## OpenAPI client sync",
      "",
      "| Client | Status | Remote spec | Detail |",
      "|---|---|---|---|",
      ...rows,
      "",
    ].join("\n"),
  );
}
