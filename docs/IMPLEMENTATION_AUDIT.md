# Implementation audit

The objective is the complete master prompt, from start to finish, with every feature verified and a simple, polished UI. It remains active. A collection of passing library tests does not satisfy the full objective.

The [full specification checklist](SPEC_CHECKLIST.md) and [requirement registry](requirements.json) preserve all 167 sections and their individual source lines. The copied specification is unchanged. The attached document supplies product requirements; its agent instructions do not override the user's instructions or execution policy.

## Evidence rules

`verified` requires direct evidence at the requirement's scope. `partial` means only a subset is proven. `unverified` means evidence is missing/indirect. `incomplete` means behavior is absent or contradicts the requirement. Do not mark a broad section verified from one narrowly scoped test.

The registry records partial section coverage and known incomplete areas. Individual source requirement lines remain unverified pending their own review. No-account sections have direct native evidence. Passing checks below prove their stated boundary only. Record commands, environment, output, fixtures/runtime identity and timestamp. Source presence is a candidate implementation, not evidence of correct runtime behavior.

## v1 acceptance gates

| # | Required behavior | Current evidence / remaining proof |
| --- | --- | --- |
| 1 | Install on Windows, macOS, Linux | Windows unsigned NSIS isolated install/uninstall passes; native debug first-run renders; clean-machine launch and macOS/Linux installation NOT TESTED |
| 2 | Polished branded setup wizard | Native wizard renders; browser 8-step walkthrough passes; broader native keyboard/scaling proof pending |
| 3 | No mandatory Conductor account | Native Skip setup opens workspace without a Conductor account; no Conductor account domain in core |
| 4 | Connect primary providers with supported authentication | API adapters and OS vault call sites; live provider connections pending |
| 5 | Claimed OAuth paths tested | No verified OAuth path; official-supported integrations still require investigation |
| 6 | API-key paths tested | Local HTTP key/rejection/redirect fixtures pass; live valid/revoked/network/rate-limit flows pending |
| 7 | Open a repository | Native IPC open/reopen preserves one canonical project identity and survives restart; native folder picker interaction pending |
| 8 | Chat with one model | Request/stream fixture tests pass; native user-to-live-provider workflow pending |
| 9 | Select a Combo | Orchestrator/engine candidates; native picker and run workflow pending |
| 10 | Clean effort control | Core rejects undeclared/highest effort; engine exact documented GPT-5 family rules repaired and tested; native UI/current catalog proof pending |
| 11 | Context compression | Bounded/deduplicated/redacted selection tests pass; full pipeline and native prompt proof pending |
| 12 | Reuse Smart Context Cache | Hash hit/change invalidation tests pass; restart/large-repo/native proof pending |
| 13 | Context Budget | Estimated token report exists; rendered/native behavior proof pending |
| 14 | Read/write under permission policy | Core Plan/approval/traversal and repaired engine Git-alias tests pass; native UI fixture verifies approval-before-write; concurrent edits and all tool boundaries pending |
| 15 | Terminal commands | Native Agent fixture command and eight Windows owned-process tests pass; separate CLI streaming lifetime and native Stop during every tool remain pending |
| 16 | Run tests | 299 workspace tests, 10 frontend unit tests, 18 browser tests and 33 native boundary checks pass at recorded revisions; broader app-driven tests pending |
| 17 | Git safely | Core preserves existing-file data and no reset exposed; native full Git workflows pending |
| 18 | Goal mode | Native scoped MCP/skill/file-write/Test Gate fixture and engine checkpoint fixture pass; full Goal contract, recovery, decisions and every tool remain pending |
| 19 | Task transfer between providers | Handoff/routing candidate and library tests; live cross-provider transfer pending |
| 20 | Continue through provider failure by policy | Structured auth/quota/outage errors verified; full fallback workflow pending |
| 21 | Install/configure MCP in app | Native custom stdio configure/initialize/tools-list/disable/export/remove and actual Agent stdio/HTTP MCP tool calls pass; real catalog installation/update and third-party authentication pending |
| 22 | Diagnose broken MCP | Native missing-executable and disabled-server diagnoses pass; broken/auth/config/repair flows pending |
| 23 | Install a skill | Native local install/list/remove, Agent/Goal instructions and installed script with literal argv/project cwd pass; trusted community source and full rollback/update lifecycle pending |
| 24 | Enable/disable Caveman | Native settings/status toggles and persistence pass; upstream update/license/redistribution lifecycle pending |
| 25 | Background Goal survives UI close | Tray/lifecycle/Goal service candidate; live close/reopen/continuation proof pending |
| 26 | Session resumes | SQLite interrupted-message recovery and native project/settings restart pass; full Goal/task/questions/tools resume pending |
| 27 | Full Access without repetitive prompts | Core authorization, browser controls and engine live-policy revocation regression pass after repair; broader native revocation proof pending |
| 28 | Fast Stop | Native Stop preserves Chat/Agent partial replies and clears approval waits after repair; actual keyboard activation and every active tool remain unverified |
| 29 | Low resource idle | Short debug process-tree diagnostic records about 548 MiB peak summed working sets and 0.024% of one core; production startup/idle/8-GB hardware targets unverified |
| 30 | Remote without Conductor account | Native loopback pinned TLS pairing/replay/scope/view-only/revocation checks pass; cross-project disclosure repaired with positive/negative native proof; LAN/full control/reconnect pending |
| 31 | Real-time remote file sync | Native hash-checked edit/local conflict and WebSocket local-change delivery pass; simultaneous writers, large files and reconnect pending |
| 32 | Verify updates before activation | Signature/integrity candidates; real download/activation/rollback proof pending |
| 33 | No plaintext secrets | Nested record/cache/settings redaction and native vault fixture omission from config/diagnostics pass; live credential lifecycle and complete persistence/export review pending |
| 34 | No known critical security issue | Git-alias and cross-project remote disclosure regressions repaired and tested; GLib unsoundness warning, integration execution and complete boundary review remain release gates |
| 35 | Another developer can build from source | Source-build docs and CI configured; clean-clone independent build pending |

## Phase coverage

All phases 0–13 from section 162 remain required. Current source candidates span foundation, desktop, providers, context, Combos, agents, Goals, integrations and remote. Computer/browser workflows, external authentication, signed releases, native lifecycle, performance and platform validation require explicit scrutiny. Do not manufacture placeholder success to close a phase.

Current command evidence is in [VERIFICATION.md](VERIFICATION.md). Shared source ownership is recorded in `.ai/COORDINATION.md`. Preserve other work; review consequential contributions before integration completion.
