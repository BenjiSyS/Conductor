# Remaining implementation and release work

The master specification remains the objective. Current builds and passing
subsets do not establish that every feature works. No production release has
been certified.

## Native repairs integrated and verified

All three failures below were reproduced, repaired, integrated with source
checks/backups and verified in the rebuilt original checkout. Native boundary
checks pass **33/33**, including actual Agent MCP/skill script/plugin execution,
Goal scoped MCP/skill/file-write/Test Gate,
positive shared-project event delivery,
cross-project event denial, scoped remote Stop, stopped Chat/Agent partial
preservation and approval cancellation cleanup. Exact run identity is in
`target/native-boundary-codex/results.json` and docs/VERIFICATION.md. Preserve
these regressions while adding runtime features; do not reapply old patches.

Run `cargo build -p conductor-app --locked`, then
`node scripts/native-boundary-smoke.mjs target/debug/conductor-app.exe`.
The harness creates its own data directory, local projects, fixture secrets,
stdio MCP process and loopback provider/TLS host. It preserves test evidence
under `target/native-boundary-codex`; it does not use real provider accounts.

1. **Cross-project remote event disclosure repaired.** Native publication
   now includes known project identity; host admission/publication fails closed
   for unknown or unshared scope. Tests check both allowed and denied events.
   Agent approvals without authorized origin remain unavailable remotely.
2. **Stopped partial responses preserved.** Chat and Agent keep emitted text
   on Stop/error; native SQLite assertions verify the stopped replies.
3. **Approval waits cancelled.** Individual Stop interrupts pending approval
   waits and removes their requests; native and engine regressions pass.

The earlier main-checkout run had 23 passing checks and three failures. These
assertions remain enabled in local tests and Windows CI. Original full checks
later passed 299 Rust tests, 18 browser tests, strict Clippy, format and frontend build. Later
source changes require fresh checks before release.

## Source gaps requiring implementation review

- Toolbox read/edit and fetch responses are bounded; edits checkpoint before
  replacement; unique exclusive temporary files avoid same-PID collisions.
  External editor conflicts, checkpoint failures and full crash recovery still
  need broader validation.
- Remote host writes are serialized with a final hash check and owned unique
  temporary files. Concurrent same-base/case-alias and large-file tests pass.
  An uncooperative editor can still write after the final check and before
  rename; this is not atomic compare-and-swap against arbitrary external writes.
- Catalog enrichment now uses documented exact model IDs, preserves provider
  metadata and leaves unknown IDs unknown. Signed catalog runtime/update wiring
  and broader current-provider metadata remain work.
- Shared Agent/Goal MCP runtime now supports scoped stdio and HTTP tool calls,
  separate integration credential lookup, live permission/config rechecks,
  bounded metadata and owned MCP process cleanup. Native Agent fixtures prove
  calls and next-turn results. Relevant skill/plugin instructions reach model
  requests, installed skill scripts and declarative plugin tools execute with
  literal argv. Tests enforce package-contained paths and permission/config
  revalidation after approval. Natural-language install/update workflows,
  legacy SSE/resumable HTTP and real third-party authentication remain work.
- Generic terminal execution's reproduced 8.128-second descendant-output hang
  is repaired through the shared owned-process guard. Eight Windows regression
  fixtures pass, including parent exit, timeout, cancellation, abort and stdin
  backpressure. Unix is uncompiled/unrun; native Stop during each integration
  tool still needs evidence. New CLI-bridge code has separate streaming
  process ownership and needs its own bounded-output/abort/credential tests;
  installed CLI presence and model listing do not prove account authentication.
- Newly added MCP OAuth, setup installers and paste expansion need bounded-input,
  credential/endpoint and lifecycle review. Local mock flows alone do not prove
  production authentication or installer success.
- Preview Center, scoped computer control, browser workflows, GitHub flows,
  integration updates and remote control UX require direct implementation and
  runtime evidence. A settings label or standalone library is insufficient.
- Global emergency shortcut defaults/registration reporting were repaired to
  Ctrl/Cmd+Alt+Shift+X. Test its real keyboard activation during work;
  invoking the emergency-stop command alone does not prove the shortcut.

## External validation gates

Live OpenAI/Anthropic/Gemini accounts, supported authentication lifecycle,
macOS/Linux installation, signed application updates/rollback and low-resource
hardware remain unverified. Windows unsigned packaging and an isolated
install/uninstall passed for an earlier development candidate. Rebuild and
revalidate an exact final source snapshot before distribution.

Cargo audit reports GLib unsoundness and two unmaintained transitive libraries.
Investigate the platform dependency path; do not suppress warnings to make the
release gate green. Cross-platform CI and package workflows are configured,
but have not run remotely from this checkout.

Source ownership and current builder findings are in `.ai/COORDINATION.md`.
The full 167-section scope and 2,418 source lines remain in
`requirements.json`; each completion claim needs evidence at its own scope.
