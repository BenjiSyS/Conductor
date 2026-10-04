# Architecture

Conductor uses Rust for durable state, provider adapters, context, permissions, orchestration, tools, remote hosting and security. Tauri provides the desktop shell using the operating system's webview. The frontend is TypeScript with Svelte and Vite. This avoids bundling a separate browser runtime while allowing a structured, accessible UI; see [Tauri's architecture](https://v2.tauri.app/start/).

## Boundaries

| Module | Responsibility |
| --- | --- |
| conductor-core | Provider-neutral domain, SQLite store, provider HTTP adapters, basic context/tool APIs |
| conductor-security | Secret detection, path guard, signatures, untrusted content, command assessment |
| conductor-context | Incremental repository index, symbols, cache, context packing, log reduction, handoffs |
| conductor-orchestrator | Combo policy, roles, routing, usage reserves, effort, Goal contract, task graph, runner |
| conductor-tools | Process execution, Git, checkpoints, MCP, packages, themes, recipes, Environment Doctor |
| conductor-remote | TLS, device pairing/trust, state channel, file-version conflict detection |
| conductor-engine | Connect core providers with context, orchestration, tools, approvals and verification |
| conductor-cli | Headless development and orchestration entry point |
| conductor-app | Native commands, OS credentials, desktop lifecycle and UI event delivery |

The security/context/orchestrator/tools/remote modules are independent libraries. Engine integrates them. Desktop and CLI consume engine. Provider-specific wire structures stay inside adapters, not persisted Goal state.

## State and secrets

SQLite records use a versioned schema, WAL and full synchronous writes. Interrupted assistant messages recover as interrupted. Future schema versions and corrupt databases fail without silently replacing user data. Non-secret records and cache strings pass secret redaction before writing. Authentication credentials use OS secure storage and are not serialized in provider metadata.

Goals, task graphs and handoffs have provider-neutral formats. Completion uses configured checks, not a model's self-report. Cancellation tokens propagate to provider and tool operations. Output/frame limits bound runaway responses.

## Untrusted content

Files, issues, webpages and integrations are data. They cannot replace system invariants or user policy. Path guards and permission checks must run in Rust at the execution boundary; frontend visibility is not authorization.

The local Laya service guides engineering model routing only. It is not part of Conductor's product authentication and does not replace deterministic evidence.

## Validation boundaries

Tests exercise individual libraries and fixture protocols. Desktop runtime, real providers, remote network conditions, update activation and platform installers each need direct evidence. See [verification](VERIFICATION.md) and [implementation audit](IMPLEMENTATION_AUDIT.md).
