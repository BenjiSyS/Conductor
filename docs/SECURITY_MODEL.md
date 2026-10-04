# Security model

Assets include user files, project decisions, provider credentials, remote device trust, tool permissions and package integrity. Attackers may control a repository file, webpage, issue, provider response, MCP server, extension, theme, download or remote client.

| Threat | Required boundary | Evidence needed |
| --- | --- | --- |
| Prompt injection | Retrieved content remains marked untrusted; execution policy independent of text | Malicious file/page/response tests and native permission workflow |
| Path escape | Canonical root guard, traversal/device/ADS checks, symlink handling | Platform-specific path tests including junctions |
| Credential exposure | OS vault, no plaintext fallback, redaction before cache/history/context | Vault smoke plus redaction fixtures and raw persisted-data inspection |
| Malicious integration | Explicit capabilities, trusted provenance, integrity verification, isolation | Invalid manifests, tamper, permission and rollback tests |
| Remote hijack | TLS, out-of-band trust, scoped devices, revocation | Wrong credentials, auth bypass, stale session, revoked connected device |
| Runaway tool | Bounded output, cancellation, timeouts, process-tree termination | Active child/descendant cancellation and timeout tests |
| Poisoned update | Signature/checksum trust, downgrade policy, healthy activation and rollback | Interrupted update, bad signature, failed health and active-Goal safe boundary |
| Lost user work | Checkpoints, conflict detection, atomic writes, no reset/discard | Interrupted write, competing changes, restore and migration tests |

These are required defenses. Module presence and passing unit tests alone do not close their complete release gates.
