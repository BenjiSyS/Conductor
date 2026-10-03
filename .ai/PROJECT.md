# Conductor

## PROJECT PURPOSE
Local-first, open-source AI orchestration desktop application. Full product specification: docs/CONDUCTOR_MASTER_BUILD_PROMPT.md. All phases remain in scope.

## STACK
Rust workspace; Tauri 2 desktop shell; TypeScript/Vite frontend; SQLite transactional persistence. Existing MIT license retained.

## IMPORTANT DIRECTORIES
- crates/conductor-core: provider-neutral domain, persistence, context, permissions, provider adapters.
- apps/desktop/src-tauri: native bridge, OS credentials, desktop lifecycle.
- apps/desktop/ui: accessible UI and design tokens.
- tests: browser verification.
- docs: specification, implementation audit, architecture, validation evidence.

## ARCHITECTURAL BOUNDARIES
Rust owns durable state, security, tools and providers. Frontend renders typed state and invokes narrow native commands. No secrets in persisted metadata or browser storage.

## IMPORTANT INTERFACES
Provider-neutral models, conversations, requests and stream events. Native bridge commands must validate inputs and authorization.

## GENERATED FILES
Cargo.lock, package-lock.json (retain); target, dist, node_modules and Tauri gen (ignored).

## DO-NOT-CASUALLY-EDIT AREAS
Permission boundaries, credential storage, persistence schema and migrations. Never discard user project changes.
