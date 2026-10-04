# Remaining implementation and release work

The master specification remains the objective. Current builds and passing
subsets do not establish that every feature works. No production release has
been certified.

## Reproduced native failures

Run `cargo build -p conductor-app --locked`, then
`node scripts/native-boundary-smoke.mjs target/debug/conductor-app.exe`.
The harness creates its own data directory, local projects, fixture secrets,
stdio MCP process and loopback provider/TLS host. It preserves test evidence
under `target/native-boundary-codex`; it does not use real provider accounts.

1. **Cross-project remote event disclosure.** A view-only device paired only
   to project A receives an Agent's output from unshared project B.
   `EngineEvent` lacks project identity; desktop publishes it unchanged;
   remote WebSocket filtering checks scope only when an event contains
   `project`. Private events must have an authorized project identity, and
   unscoped private events must fail closed. Remote inbound controls also
   need authorization against the referenced project/task/approval, rather
   than `can_control` alone. Review before exposing a host outside loopback.
2. **Stopped chat loses its partial response.** HTTP cancellation succeeds,
   but the stopped assistant record in SQLite has empty text. Preserve
   streamed text on cancellation and provider errors, then verify reopening.
3. **Conversation Stop leaves an approval waiter running.** An Agent waiting
   for file-write approval stays pending after its token is cancelled.
   Cancellation must interrupt approval waits and remove the pending request.
   Emergency Stop currently releases this wait; individual Stop must do so.

The latest native boundary run has 23 passing checks and these three failures.
The failing assertions remain enabled in local tests and Windows CI.

## Source gaps requiring implementation review

- `edit_file` in engine Toolbox still writes directly without the checkpoint
  path used by other mutations. Its reads are unbounded. The atomic helper
  uses one temporary filename per PID, so concurrent writes can interfere.
- Remote sync checks a hash and then writes without one serialized operation.
  Its temporary filename also uses only the PID. Concurrent remote/local
  edits need deterministic conflict and crash-safety tests.
- Built-in catalog enrichment applies the original GPT-5 effort levels to
  every `gpt-5` prefix, including families with different documented levels.
  Use documented model-specific rules; leave unknown models unknown.
- Installing/listing an MCP, skill or plugin does not prove Agent execution
  integration. Toolbox currently lists only built-in filesystem, search,
  command and Git tools. Review shared MCP/skill/plugin execution, browser and
  computer-use dispatch against the full specification.
- Preview Center, scoped computer control, browser workflows, GitHub flows,
  integration updates and remote control UX require direct implementation and
  runtime evidence. A settings label or standalone library is insufficient.
- Global emergency shortcut defaults/registration reporting are being repaired
  in shared desktop source. Test its real keyboard activation during work;
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
