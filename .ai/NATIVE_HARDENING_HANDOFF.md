# Native hardening integration

Reviewed patch: `target/native-hardening-integration.patch`.
Base hashes: `target/native-hardening-patch-identities.json`.
Source: `C:\Users\benji\.codex\worktrees\native-hardening\Conductor`.

The reviewed patch has already been applied to the original checkout. Do not
reapply it or replace whole files from the snapshot. Root integration preserved
concurrent features, including native usage recording.

Prior files are backed up under
`target/native-integration-backup-edec69d2-4c6b-4919-9439-d2a7764395f7`.
The later three-file catalog/toolbox/sync patch was also applied, with backup
`target/file-integration-backup-f813d9d8-c6f5-4ca9-b48c-2186fbd5c55e`.

Repairs: cancellation-aware approval waits; dropped approval cleanup and
insert-before-notify ordering; preserved stopped/error Chat and Agent replies;
project identities on native engine events; fail-closed remote event scope;
inbound project authorization; project-scoped remote Stop and resource checks.

Verified in the isolated checkout on 2026-10-03 at 23:22 UTC: native boundary
harness **28/28**, core **44/44**, engine **18/18**, remote **7 unit + 3
integration**, strict core/engine/remote Clippy, native build and frontend build.
Native executable SHA256:
`86ab39dc6605ff320c9a1973719a459935417b80fbc7c318b43c652bd51276a3`.
Raw native evidence is in the isolated checkout's
`target/native-boundary-codex/results.json`.

After original integration and rebuild, the native harness passed **28/28** at
`2026-10-03T23:48:59.299Z`, executable SHA256
`8082ab63bc5b726b56170a3f4b218d4d8fca7a3f5ac4c7da660053e1e7cc6e2b`.
Original full workspace verification passed **233 tests**, strict all-target
Clippy, Rust/frontend formatting and frontend build. The original browser suite
separately passed **16/16**. These runs predate later runtime additions and the
watcher debounce correction. They do not prove live providers, OS keyboard
activation, other platforms or every master requirement.

Agent approvals without goal/project origin deliberately remain unavailable
remotely until their origin can be authorized. Remote prompt/answer dispatch
still needs complete UX and typed resource validation before broad claims.

## Runtime integration — 2026-10-04

`target/native-runtime-integration.patch` has also been applied with fresh patch
checks and source identity guards. Do not reapply it. Prior files and lock/UI
backup: `target/runtime-integration-backup-99873564-c437-425e-bf49-76b3ba23ca9c`.
Original usage recording and subscriptions module were preserved.

Original workspace **269 tests**, strict all-target Clippy, Rust/frontend
formatting, frontend/debug app build and **17 browser tests** PASS. Rebuilt
original native app passes **31/31** at `2026-10-04T09:21:14.721Z`, SHA256
`7858ef8fce02b8c9e4ce5f8aabbc4008c02ba33f71dd98746ee97d7c027e1c42`.
Raw evidence: `target/native-boundary-codex/results.json`. Cold harness frontend
startup and cleanup passed. The new checks exercise Agent stdio/HTTP MCP,
integration vault values, relevant skills and literal-argv plugin tools.
Windows MCP lifetime fixtures pass; Unix implementation remains uncompiled and
unrun. Skill commands/scripts, full integration updates and supported real
OAuth remain separate gates. Later concurrent source additions require fresh
checks; this is not certification of every master requirement.

## Skill commands and general execution — 2026-10-04

Reviewed general execution and skill-command patches are integrated. Do not
reapply `target/exec-lifetime-integration.patch` or
`target/skill-command-integration.patch`. Prior bytes are preserved under
`target/exec-lifetime-integration-backup-f41bf1eb-506a-4892-b2b3-5423c527e4cf`
and `target/skill-command-integration-backup-da5c248b-10d9-4782-83e1-bc504cf5fd83`.
Public command_for and the original CLI/subscription additions were retained.

Original workspace **299 tests**, strict all-target Clippy, Rust/frontend
formatting, frontend/debug build and **18 browser tests** passed. Native
**33/33** passed at `2026-10-04T10:10:34.378Z`, SHA256
`100bfd692c1dfc784d7efc34f96d9701c56f58fcbb0cf5f2f94457029d5b24b0`.
New native evidence covers installed skill scripts, literal argv and the
project cwd. Windows generic exec now uses owned-tree cleanup; eight lifetime
fixtures pass. Unix remains uncompiled/unrun. CLI streaming has a separate
execution path; no live CLI authentication was performed.

This native run reused another builder's existing Vite server and left it
running. Cold startup/owned-server cleanup is proven only by the earlier
31-check run. Future reports record frontend ownership explicitly. New OAuth,
setup and paste source needs separate review. No full-product completion claim.
