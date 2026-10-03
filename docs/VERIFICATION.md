# Verification record

## 2026-10-03 — Windows local development

Execution environment: Windows PowerShell in `D:\Conductor`; Rust 1.98.1, Cargo 1.98.1, Node.js 26.7.0. This is a developer machine, not an 8-GB hardware profile or cross-platform test.

| Command | Result | What it proves |
| --- | --- | --- |
| `cargo test -p conductor-core` | PASS: 36 tests | Core checks include the repaired engine PathGuard Git-alias integration regression and finite-page catalog guard |
| `cargo test --workspace --quiet` | PASS: 195 tests | Workspace libraries, desktop test target and CLI compile/test; does not validate all native UI or live external providers |
| `cargo clippy -p conductor-core --all-targets --no-deps -- -D warnings` | PASS | Core library/test lint only; dependency and desktop lint excluded |
| `cargo clippy -p conductor-core --all-targets -- -D warnings` | PASS after dependency repair | Core plus dependencies lint clean; prior security byte_char_slices failure resolved |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS | Prior context type_complexity failures resolved; current workspace lint clean at that revision |
| `cargo fmt --all -- --check` | FAILED after subsequent shared edits | main.rs, prefs.rs, toolbox.rs and paths.rs need formatting in the owning builder's source |
| `npm run format:check` | FAILED after subsequent shared edits | App.svelte, settings/General.svelte and lib/mock.ts need formatting |
| `npm run build` | PASS: zero Svelte errors/warnings | Frontend type checking and production bundling on Windows |
| `npm audit --json` | PASS: zero reported vulnerabilities | Installed npm dependency audit at that revision |
| `cargo build -p conductor-app --locked` | PASS | Windows debug desktop compilation, not production packaging |
| `npx tauri build --config .github/unsigned-tauri.json --bundles nsis -- --locked` | PASS: optimized build and 1 NSIS bundle | Windows unsigned development installer generated (7,103,410 bytes); installation, signatures and later source edits remain separate checks |
| NSIS isolated silent install/uninstall | PASS | Current-user installation into target/installer-smoke, files and version registration checked, then removed. No app launch, shortcut creation or user-data removal requested. |
| `cargo run -p conductor-cli -- --data-dir target/smoke-state doctor --json` | PASS | CLI Environment Doctor detects local tools without installing anything |
| `npm run test:e2e -- --output=target/playwright-codex --workers=1` | PASS: 14 tests | Browser preview controls; all three prior UI regressions pass against repaired source. Native behavior remains separate. |
| Focused Playwright retest of Stop, Combo save and panels | PASS: 3 tests | All three earlier browser failures pass against updated source; separate output under target/playwright-codex |
| `npm test -- --reporter=json --outputFile=target/frontend-unit-codex.json` | PASS: 10 tests | Keyboard modifiers, display formatting, model target fallback/Local First and exclusive panels. Report moved from Vitest's UI-root-relative path to target/frontend-unit-codex.json. |
| `node scripts/native-boundary-smoke.mjs target/debug/conductor-app.exe` | FAILED: 23 pass, 3 fail | Real Windows IPC, vault, local package/MCP lifecycle, pinned loopback TLS, file conflicts, WebSocket changes and restart; reproduces partial-response loss, approval-wait Stop failure and cross-project remote event disclosure |
| `node --test scripts/release-evidence.test.mjs` | PASS: 2 tests | Known SHA-256 fixture, accurate sizes/paths, honest unsigned metadata, stable regeneration and empty-output refusal |
| `cargo audit --json` | 0 listed vulnerabilities; 3 warnings | RUSTSEC-2024-0429 GLib 0.18.5 unsoundness plus unmaintained proc-macro-error and rustls-pemfile. Not a clean security bill. |
| Engine PathGuard Git-alias regression in core tests | PASS after security repair | .GIT/config and ambiguous Windows aliases are refused for writes before and after Git initialization |

Core tests specifically cover: unknown/highest effort rejection, each provider JSON/SSE protocol, local bearer header, usage delivery, invalid key, rate limit, cancelled request, redirect credential isolation, bounded catalog, split UTF-8 at every byte, CRLF/multiline SSE, same-chunk partial output before error, undelimited/incomplete response, provider token-limit/filtered partial output, JSON-safe nested redaction, cache/settings redaction, matching history identity, future schema refusal and corrupted-database preservation. Context notices fit the byte budget; edits beyond the selected prefix invalidate cached content.

Latest additional provider checks pass: cumulative usage across split Anthropic input/output frames, overflowing token totals rejected, total streamed response bounded at 2 MB, Anthropic/Gemini catalog pagination and declared metadata, encoded cursors, aggregate/page/model limits, repeated/malformed cursor rejection and cancellation. The PathGuard integration regression now passes and remains enabled. Library success does not close the three reproduced native failures.

Unsigned NSIS artifact SHA-256: `705e85403ee3f708683f0214523d8cf940d6a9619228ab7bae9483fe4e227578`. Package evidence lives under `target/release/bundle`. Source changed during release compilation; this artifact is a development candidate, not certification of the latest checkout.

Installer smoke evidence: `target/installer-smoke-result.json`. Installation and uninstallation returned exit code 0. The installed executable differs from the unbundled build only in Tauri's documented bundle marker (`__TAURI_BUNDLE_TYPE_VAR_NSS` versus `__TAURI_BUNDLE_TYPE_VAR_UNK`); after normalizing that marker, every byte matches. [Tauri bundler source](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle.rs) patches the executable per package and restores the unbundled copy afterward. `Get-AuthenticodeSignature` confirms the development installer is NotSigned. This does not prove SmartScreen behavior, first launch from an installed path, shortcuts, in-place upgrades, user-data deletion/preservation, signed updates or clean-machine prerequisites.

Native app launched with `CONDUCTOR_DATA_DIR=D:\Conductor\target\native-smoke-state`. The 1240×820 first-run wizard rendered; Skip setup opened the workspace. UI Automation exposed labelled controls after input. Project folder selection yielded Obsidian during shared GUI input; the intended Conductor selection sequence is not verified. Native interaction paused to avoid conflicting input from another builder/user.

Native log `target/native-smoke-state/logs/conductor.2026-10-03.log` reports `HotKey already registered` for Ctrl+Shift+Escape. Emergency stop registration failed and must be fixed before claiming it works. A responsive app process alone does not prove whole-app memory/idle performance.

### Native boundary integration evidence

`scripts/native-boundary-smoke.mjs` launches a freshly built debug executable with a separate data/WebView folder. It uses generated local projects and fixture secrets, an authenticated loopback OpenAI-compatible HTTP server, a stdio MCP fixture and a TLS host whose fingerprint is checked against native IPC. An isolated headless Chrome client exercises the host's WebSocket. No live account, public tunnel or existing user data is used. Exact executable SHA-256, fixture directory, timestamp and per-step results are in `target/native-boundary-codex/results.json`.

Passing checks prove MCP missing-dependency diagnosis and stdio initialization/tools listing; disable/export/remove; local skill/plugin/theme installation/removal and executable theme rejection; Caveman settings persistence; custom-provider connection and OS vault omission from exports; config round trip/rejection; diagnostics omission; one-use pairing, project-scoped HTTP access, file hashes/conflicts, sensitive-path and Git-write guards, view-only write denial, local file-change WebSocket delivery, immediate HTTP/WebSocket revocation, remote stop, and native project/settings persistence after restart.

Three failing assertions remain enabled: cancelling a chat loses the streamed partial reply in SQLite; individual Stop leaves an Agent waiting for permission; a remote device scoped only to one project receives another project's Agent output. Emergency Stop releases the approval wait but does not repair individual Stop. The cross-project event leak blocks release readiness. Details and source boundaries are in [REMAINING_WORK.md](REMAINING_WORK.md).

The other builder's `target/screenshots/native-smoke.json` records 11 successful desktop UI steps against a scripted local provider. Its script and assertions were reviewed: native wizard/provider setup, project open, chat, approval-before-write, checkpoints, Goal questions/review/Test Gate, Environment Doctor and unpaired TLS rejection. This is useful integration evidence, not a live cloud account, all-tools, full remote control or global-keyboard proof.

### 2026-10-04 — process-tree diagnostic

Ten samples of the existing debug app and its descendants (8 processes, including WebView2 and conhost) covered 130.039 seconds. Peak summed working sets: 574,332,928 bytes (about 548 MiB). Cumulative CPU increased 0.03125 seconds (about 0.024% of one CPU core over this interval). Raw samples: `target/native-smoke-state/idle-process-tree.json`. Host physical memory: 34,268,213,248 bytes (about 31.9 GiB).

This is a short diagnostic on a shared developer machine during other build activity. Working sets double-count shared pages, sampling can miss short-lived child processes, and a debug build is not the release build. It does not establish cold startup, battery impact, 8-GB usability, large-repository indexing or an idle-memory target.

## Required remaining validation

- Re-run full workspace format/lint/build/tests after shared integration stabilizes.
- Produce the production desktop build; broaden native runtime inspection beyond first-run rendering.
- Test all primary UI flows, light/dark/reduced motion, keyboard navigation, high-DPI and small resolutions.
- Verify API credentials with live test accounts through native OS vault storage. No OAuth path is verified.
- Verify native Agent/Goal tools, configured tests, fallback, Stop, emergency stop and durable background resume.
- Verify MCP installation/doctor, skills/plugins/Caveman/theme updates with trusted real sources and rollback.
- Verify remote encrypted LAN access, synchronization, pairing and revoked sessions.
- Verify browser/computer-use scope, preview/tunnel lifecycle, GitHub workflows and permission boundaries.
- Produce and test platform installers and signed update/rollback paths.
- Measure startup phases, idle CPU/RAM, indexing and prompt latency; run a low-resource profile.
- Audit each source requirement in requirements.json. Passing test counts cannot replace this review.

No fabricated OAuth, live provider, remote, update, cross-platform, benchmark or complete-product claims are made here.
