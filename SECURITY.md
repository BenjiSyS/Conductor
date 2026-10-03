# Security

Conductor can operate files, tools, integrations and remote sessions. Treat it as a local automation platform with explicit trust boundaries.

## Report a vulnerability

Use the repository's private vulnerability reporting feature when enabled. If unavailable, contact the maintainer through an established private channel. Do not post credentials, working exploits against users, or private project data publicly. No dedicated security email or response SLA is currently configured.

Include affected versions, exact reproduction steps, impact, platform, and redacted logs. Security-sensitive changes require a focused review and deterministic regression tests.

## Boundaries

- Provider keys belong in Windows Credential Manager, macOS Keychain or Linux Secret Service. No plaintext credential fallback.
- Retrieved files, webpages, model responses and integration packages are untrusted data.
- Tool policy is enforced in Rust. Plan mode does not gain write permission through Full Access.
- Project paths must remain within their permitted root; traversal, symlink escapes and Windows special paths need regression coverage.
- Downloads require verified provenance and applicable checksum/signature checks before activation. Permission level does not disable integrity checks.
- Remote access requires authenticated encrypted connections, project scope, explicit pairing and revocation.
- Themes are validated data; they cannot execute scripts or arbitrary CSS.
- Logs, context, history and cached content are redacted and bounded. Detection cannot guarantee every unknown credential is found.

See [the security model](docs/SECURITY_MODEL.md) and [verification record](docs/VERIFICATION.md). There is no released version with a completed security audit yet.
