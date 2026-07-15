# Security policy

## Supported versions

Security fixes are made on the latest released minor version. Users should
upgrade to the newest `openapi-to-rust` release before reporting or verifying a
fix; older pre-1.0 minors do not receive backports by default.

## Reporting a vulnerability

Do not open a public issue. Use GitHub's
[private vulnerability-reporting form](https://github.com/gpu-cli/openapi-to-rust/security/advisories/new)
and include:

- the affected `openapi-to-rust` version or commit;
- whether the issue affects the CLI, remote-spec fetching, generated source,
  generated HTTP clients, or generated Axum servers;
- a minimal document or reproduction when safe to share;
- the expected impact and any known mitigations.

Maintainers will acknowledge a complete report within seven days when
possible, keep the reporter informed as the impact is assessed, and coordinate
disclosure after a fix or mitigation is available. Please allow a reasonable
remediation window before publishing details.

## Security boundaries

OpenAPI documents are untrusted input. Remote direct generation accepts HTTPS
sources (plus loopback development), rejects embedded credentials and
redirects, and applies response-size and timeout limits. Generated code must
still be reviewed like any other source dependency before it is committed or
executed.

The generator does not provide runtime schema validation. Constraints emitted
as documentation are not security checks, and generated applications remain
responsible for authentication, authorization, input limits, TLS credentials,
and business-rule validation.
