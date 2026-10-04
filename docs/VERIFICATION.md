# Verification record

## 2026-10-03 — Windows local development

Execution environment: Windows PowerShell in `D:\Conductor`; Rust 1.98.1, Cargo 1.98.1, Node.js 26.7.0. This is a developer machine, not an 8-GB hardware profile or cross-platform test.

| Command | Result | What it proves |
| --- | --- | --- |
| `cargo test -p conductor-core` | PASS: 45 tests | Core checks include provider image serialization/validation, image persistence without text-redaction corruption, PathGuard aliases and bounded catalogs |
| `cargo test --workspace --locked --quiet` | PASS: 299 tests | Original checkout after reviewed skill-command and general exec integration; includes 8 general exec lifetime, 6 MCP lifetime and 9 transport integration tests; not every native feature or live provider |
| `cargo clippy -p conductor-core --all-targets --no-deps -- -D warnings` | PASS | Core library/test lint only; dependency and desktop lint excluded |
| `cargo clippy -p conductor-core --all-targets -- -D warnings` | PASS after dependency repair | Core plus dependencies lint clean; prior security byte_char_slices failure resolved |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS | Prior context type_complexity failures resolved; current workspace lint clean at that revision |
| `cargo fmt --all -- --check` | PASS after integration | Rust workspace formatting at the checked revision |
| `npm run format:check` | PASS after integration | Frontend formatting at the checked revision |
| `npm run build` | PASS: zero Svelte errors/warnings | Frontend type checking and production bundling on Windows |
| `npm audit --json` | PASS: zero reported vulnerabilities | Installed npm dependency audit at that revision |
| `cargo build -p conductor-app --locked` | PASS | Windows debug desktop compilation, not production packaging |
| `npx tauri build --config .github/unsigned-tauri.json --bundles nsis -- --locked` | PASS: optimized build and 1 NSIS bundle | Windows unsigned development installer generated (7,103,410 bytes); installation, signatures and later source edits remain separate checks |
| NSIS isolated silent install/uninstall | PASS | Current-user installation into target/installer-smoke, files and version registration checked, then removed. No app launch, shortcut creation or user-data removal requested. |
| `cargo run -p conductor-cli -- --data-dir target/smoke-state doctor --json` | PASS | CLI Environment Doctor detects local tools without installing anything |
| `npx playwright test --workers=1 --output=target/playwright-codex` | PASS: 18 tests | Browser preview controls, Usage settings, mocked CLI connection UI, GitHub tabs and structured preview feedback; native behavior remains separate. |
| Focused Playwright retest of Stop, Combo save and panels | PASS: 3 tests | All three earlier browser failures pass against updated source; separate output under target/playwright-codex |
| `npm test -- --reporter=json --outputFile=target/frontend-unit-codex.json` | PASS: 10 tests | Keyboard modifiers, display formatting, model target fallback/Local First and exclusive panels. Report moved from Vitest's UI-root-relative path to target/frontend-unit-codex.json. |
| `node scripts/native-boundary-smoke.mjs target/debug/conductor-app.exe` | PASS: 33 checks after skill-command/general exec integration and rebuild | Windows IPC/vault, actual Agent MCP/skill script/plugin execution, Goal MCP/skill/file-write/Test Gate, pinned loopback TLS, file conflicts, scoped WebSocket/Stop and restart. Latest run reused an existing frontend server; earlier 31-check run separately verified cold startup/owned cleanup. |
| `node --test scripts/release-evidence.test.mjs` | PASS: 2 tests | Known SHA-256 fixture, accurate sizes/paths, honest unsigned metadata, stable regeneration and empty-output refusal |
| `cargo audit --json` | 0 listed vulnerabilities; 3 warnings | RUSTSEC-2024-0429 GLib 0.18.5 unsoundness plus unmaintained proc-macro-error and rustls-pemfile. Not a clean security bill. |
| Engine PathGuard Git-alias regression in core tests | PASS after security repair | .GIT/config and ambiguous Windows aliases are refused for writes before and after Git initialization |

Core tests specifically cover: unknown/highest effort rejection, each provider JSON/SSE protocol, local bearer header, usage delivery, invalid key, rate limit, cancelled request, redirect credential isolation, bounded catalog, split UTF-8 at every byte, CRLF/multiline SSE, same-chunk partial output before error, undelimited/incomplete response, provider token-limit/filtered partial output, JSON-safe nested redaction, cache/settings redaction, matching history identity, future schema refusal and corrupted-database preservation. Context notices fit the byte budget; edits beyond the selected prefix invalidate cached content.

Latest additional provider checks pass: cumulative usage across split Anthropic input/output frames, overflowing token totals rejected, total streamed response bounded at 2 MB, Anthropic/Gemini catalog pagination and declared metadata, encoded cursors, aggregate/page/model limits, repeated/malformed cursor rejection and cancellation. The PathGuard integration regression remains enabled. The three reproduced native failures were separately closed by original-checkout native tests below.

Unsigned NSIS artifact SHA-256: `705e85403ee3f708683f0214523d8cf940d6a9619228ab7bae9483fe4e227578`. Package evidence lives under `target/release/bundle`. Source changed during release compilation; this artifact is a development candidate, not certification of the latest checkout.

Installer smoke evidence: `target/installer-smoke-result.json`. Installation and uninstallation returned exit code 0. The installed executable differs from the unbundled build only in Tauri's documented bundle marker (`__TAURI_BUNDLE_TYPE_VAR_NSS` versus `__TAURI_BUNDLE_TYPE_VAR_UNK`); after normalizing that marker, every byte matches. [Tauri bundler source](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle.rs) patches the executable per package and restores the unbundled copy afterward. `Get-AuthenticodeSignature` confirms the development installer is NotSigned. This does not prove SmartScreen behavior, first launch from an installed path, shortcuts, in-place upgrades, user-data deletion/preservation, signed updates or clean-machine prerequisites.

Native app launched with `CONDUCTOR_DATA_DIR=D:\Conductor\target\native-smoke-state`. The 1240×820 first-run wizard rendered; Skip setup opened the workspace. UI Automation exposed labelled controls after input. Project folder selection yielded Obsidian during shared GUI input; the intended Conductor selection sequence is not verified. Native interaction paused to avoid conflicting input from another builder/user.

Historical native log `target/native-smoke-state/logs/conductor.2026-10-03.log` reports `HotKey already registered` for Ctrl+Shift+Escape. The default and registration reporting were subsequently repaired to Ctrl/Cmd+Alt+Shift+X. Real keyboard activation during work remains unverified. A responsive app process alone does not prove whole-app memory/idle performance.

### Native boundary integration evidence

`scripts/native-boundary-smoke.mjs` launches a freshly built debug executable with a separate data/WebView folder. It uses generated local projects and fixture secrets, an authenticated loopback OpenAI-compatible HTTP server, a stdio MCP fixture and a TLS host whose fingerprint is checked against native IPC. An isolated headless Chrome client exercises the host's WebSocket. No live account, public tunnel or existing user data is used. Exact executable SHA-256, fixture directory, timestamp and per-step results are in `target/native-boundary-codex/results.json`.

Passing checks prove MCP missing-dependency diagnosis and stdio initialization/tools listing; disable/export/remove; local skill/plugin/theme installation/removal and executable theme rejection; Caveman settings persistence; custom-provider connection and OS vault omission from exports; config round trip/rejection; diagnostics omission; one-use pairing, project-scoped HTTP access, file hashes/conflicts, sensitive-path and Git-write guards, view-only write denial, local file-change WebSocket delivery, immediate HTTP/WebSocket revocation, remote stop, and native project/settings persistence after restart.

The main-checkout run reproduced three failures: stopped chat partial loss, individual Stop leaving an approval waiter, and cross-project Agent output disclosure. Reviewed repairs were integrated with backups and fresh patch/source identity checks. The earlier rebuilt original checkout passed 28/28 at `2026-10-03T23:48:59.299Z`, SHA256 `8082ab63bc5b726b56170a3f4b218d4d8fca7a3f5ac4c7da660053e1e7cc6e2b`; that report is preserved as `target/native-boundary-pre-runtime-results.json`. Shared-project output remains delivered; remote Stop cancels its authorized project; unshared output and cross-project/missing-scope Stop are refused. Stopped Chat and Agent partial replies remain in SQLite.

After MCP/skill/plugin runtime integration and the watcher debounce correction, the rebuilt **original checkout passed 31/31 native checks at `2026-10-04T09:21:14.721Z`**, executable SHA256 `7858ef8fce02b8c9e4ce5f8aabbc4008c02ba33f71dd98746ee97d7c027e1c42`. Raw evidence is `target/native-boundary-codex/results.json`. New checks prove actual Agent stdio and HTTP MCP calls with integration credentials, selected skill instructions reaching the provider request, tool output reaching the next model turn, and declarative plugin execution with literal argv and installed instructions. The provider and MCP servers are local fixtures. The harness started its own Vite preview from built assets with no server already listening on port 1420; the port was closed after cleanup. Full workspace 269 tests, strict all-target Clippy, Rust/frontend formatting, frontend build (zero Svelte errors/warnings) and 17 browser tests passed before this native run. Later concurrent source additions still require their own fresh checks.

Windows MCP process tests prove owned child/descendant cleanup on shutdown, Drop, owner abort, naturally exited parent and runtime shutdown, while preserving an unrelated process. Unix process-group cleanup is source-reviewed but NOT COMPILED OR RUN. HTTP MCP fixtures cover JSON/SSE, session headers, pagination, bounded reads, cancellation and secret redaction. Resumable GET/legacy SSE, real third-party MCP authentication and remote-side cancellation guarantees remain unverified or unsupported. Natural-language integration installation/update and full native Stop coverage remain separate work.

The same binary then passed **32/32** at `2026-10-04T09:33:36.025Z`, adding a native Goal fixture: scoped HTTP MCP with OS credentials, selected installed skill, tool output reaching the next request, file write and passing `node --check` Test Gate. A separate engine GoalService test also proves a Git checkpoint before work. The first expanded harness attempt was 31/32 because its state comparison used Rust variant spelling instead of the API's lowercase serialized names; correcting only that assertion passed. Prior reports are preserved at `target/native-boundary-runtime31-results.json` and `target/native-boundary-goal-state-fixture-failure.json`.

The latest rebuilt original binary passed **33/33** at `2026-10-04T10:10:34.378Z`, SHA256 `100bfd692c1dfc784d7efc34f96d9701c56f58fcbb0cf5f2f94457029d5b24b0`. Fixture directory: `target/native-boundary-codex/run-kN6KtQ`. New evidence proves installed skill script dispatch with literal shell-looking arguments, package-contained script resolution and the correct project working directory. Instructions and script output reach subsequent fixture model requests. This run reused another builder's existing Vite server; it did not test cold frontend startup or remove that server. Future harness reports explicitly record frontend ownership.

General exec previously reproduced an 8.128-second descendant-output hang with a one-second timeout. The reviewed owned-process fix is now integrated. Eight Windows integration tests prove parent-exit cleanup/status/output, timeout, cancellation, owner abort, backpressured stdin, bounded output and unrelated-process preservation. Skill/plugin commands share this executor. Unix cleanup remains uncompiled/unrun; CLI-bridge streaming uses a separate execution path and needs its own review.

Full original workspace **299 tests**, strict all-target Clippy and Rust/frontend formatting passed. Frontend build reported zero Svelte errors/warnings; **18 browser tests** passed, including mocked CLI-connect UI. The latest native binary includes CLI bridge code, but native bridge authentication was not exercised. CLI help on this machine confirms installed `agy`, `codex exec` and `claude` print/JSON flags; installed presence and model listing do not prove signed-in accounts. New OAuth/setup/paste source added by the other builder needs separate review and fresh checks.

The other builder's `target/screenshots/native-smoke.json` records 11 successful desktop UI steps against a scripted local provider. Its script and assertions were reviewed: native wizard/provider setup, project open, chat, approval-before-write, checkpoints, Goal questions/review/Test Gate, Environment Doctor and unpaired TLS rejection. This is useful integration evidence, not a live cloud account, all-tools, full remote control or global-keyboard proof.

### 2026-10-04 — process-tree diagnostic

Ten samples of the existing debug app and its descendants (8 processes, including WebView2 and conhost) covered 130.039 seconds. Peak summed working sets: 574,332,928 bytes (about 548 MiB). Cumulative CPU increased 0.03125 seconds (about 0.024% of one CPU core over this interval). Raw samples: `target/native-smoke-state/idle-process-tree.json`. Host physical memory: 34,268,213,248 bytes (about 31.9 GiB).

This is a short diagnostic on a shared developer machine during other build activity. Working sets double-count shared pages, sampling can miss short-lived child processes, and a debug build is not the release build. It does not establish cold startup, battery impact, 8-GB usability, large-repository indexing or an idle-memory target.

## Required remaining validation

- Preserve current workspace/native regressions and re-run relevant checks after subsequent source changes.
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
