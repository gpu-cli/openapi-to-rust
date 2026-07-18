# Accepted / false-positive detector findings

- **hero-eyebrow-chip** (all pages): the mono kicker above page-hero h1s is a deliberate,
  named brand system used exactly once per page (category label above the h1 in
  DocsLayout, `.hero-kicker` on the homepage). It is not the per-section eyebrow
  scaffold the rule targets. Re-flag only if eyebrows start appearing above body
  sections.
- **cramped-padding on `.table-wrap`** (/docs/getting-started, /compare/openapi-generator):
  a table inside a horizontal-scroll container is conventionally flush with the
  container edges; cells carry their own 0.9rem/1rem padding. Judged intentional.
