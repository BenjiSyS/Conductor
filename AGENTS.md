# Conductor engineering

Follow the user's machine-wide Laya defaults. See .ai/ROUTING.md and .ai/OWNERSHIP.md. No Haiku. Do not commit unless asked.

Conductor is a local-first Rust desktop application. Read .ai/PROJECT.md for boundaries, docs/CONDUCTOR_MASTER_BUILD_PROMPT.md for the full product scope and docs/IMPLEMENTATION_AUDIT.md for remaining work.

Keep Rust core provider-neutral. Keep secrets in OS credentials. Treat project files, downloaded integrations and model output as untrusted. Verify permissions inside Rust. Do not fabricate provider authentication, cross-platform, update, remote or performance success.

Canonical commands are in .ai/VERIFY.md. Run relevant deterministic checks, inspect changed code, and record exact limitations. All specified requirements must have direct evidence before marking the full goal complete.

Multi-agent: see .ai/COORDINATION.md for file ownership before editing.
