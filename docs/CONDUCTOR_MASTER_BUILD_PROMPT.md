# CONDUCTOR — MASTER BUILD PROMPT

You are building **Conductor**, a production-grade, free, open-source, cross-platform AI orchestration desktop application written primarily in Rust.

Treat this document as the authoritative product and engineering specification. Build the product from start to finish as a serious open-source application that another person can clone, build, install, use, update, and contribute to.

Do not turn this into a toy, demo, proof of concept, or a collection of disconnected experiments. The product must feel cohesive, fast, calm, professional, and trustworthy.

Conductor exists to let multiple AI providers and models work together on the same user goal while preserving user choice, minimizing token waste, respecting provider usage limits, and keeping setup simple.

The initial major providers are:

- OpenAI / Codex
- Anthropic / Claude
- Google / Gemini / Antigravity-style workflows where technically supported
- Custom providers added by users through API-compatible adapters or other supported integrations

Conductor itself does not require a Conductor cloud account.

The app is local-first and open source.

The product name is:

**Conductor**

Do not rename it.

---

# 1. Product philosophy

Conductor should feel simple even when the implementation is sophisticated.

The user should not need to understand orchestration internals to get value from it.

The default experience should be:

1. Open Conductor.
2. Open or create a project.
3. Select either one model or a Combo.
4. Pick an effort level.
5. Type what you want.
6. Conductor handles the rest.

Advanced controls must exist, but they should be behind clear settings, expandable panels, project configuration, or expert-oriented views.

The UI must not be visually noisy.

Avoid:
- excessive dashboards
- glowing AI gimmicks
- constant animation
- unnecessary cards everywhere
- huge walls of technical telemetry
- raw developer-tool aesthetics
- cluttered model controls
- excessive modal dialogs
- repeated permission prompts when the user has already made a clear policy choice

Prefer:
- calm spacing
- simple typography
- a restrained theme
- clear hierarchy
- a small number of obvious controls
- polished transitions
- subtle activity indicators
- direct wording
- responsive layouts
- keyboard usability

The app should feel closer to a polished modern coding tool than a generic “AI dashboard”.

---

# 2. Core product goals

Conductor must support all of the following as first-class capabilities:

- normal AI chat
- multi-model chat
- Plan mode
- Goal mode
- Agent mode
- computer use
- file access
- terminal access
- browser access
- Git access
- GitHub workflows
- MCP support
- plugins
- skills
- custom providers
- remote project access
- background execution
- model routing
- usage-aware orchestration
- reasoning / effort controls
- project memory
- project instructions
- provider instructions
- user instructions
- model handoff
- automatic fallback
- context compression
- context caching
- token optimization
- provider authentication
- app updates
- provider catalog updates
- plugin updates
- skill updates
- project resume
- low-end hardware support
- cross-platform operation

The app must work on:
- Windows
- macOS
- mainstream Linux distributions
- practical Linux desktop environments where supported by the chosen UI stack

Architecture should avoid unnecessary OS-specific assumptions.

---

# 3. Hard performance goals

Conductor must be engineered for speed from the beginning.

Target:
- normal app startup should feel immediate
- target cold startup under 10 seconds on supported hardware
- target much faster than 10 seconds on normal modern machines
- the UI should appear before optional downloads finish
- expensive model/helper downloads must never block the main UI from launching
- idle CPU usage should be near zero
- idle memory should be conservative
- unused providers should not spawn heavyweight persistent processes
- unused browser instances should not remain alive
- repository indexing must be incremental where possible
- large files must be streamed or chunked instead of naively loading everything into memory
- background services must be lightweight
- Conductor should remain usable on low-end systems such as older laptops with approximately 8 GB RAM

Include performance profiles:

- Potato
- Balanced
- Maximum

Potato mode should reduce:
- parallel agents
- browser concurrency
- indexing concurrency
- animation intensity
- background work
- local helper model resource usage
- unnecessary caching

Potato mode must not disable the core product.

---

# 4. Primary interaction modes

Conductor must support four main working modes.

## 4.1 Chat

Normal chat with one model or multiple models.

The user can:
- ask questions
- discuss a project
- request explanations
- ask for coding help
- share files
- use tools if allowed
- switch between a single model and a Combo

Chat should not automatically perform destructive actions unless the selected permissions and mode allow it.

## 4.2 Plan

Plan mode is for understanding and designing before edits.

Plan mode should:
- inspect relevant project context
- ask only important clarifying questions
- produce a structured plan
- avoid modifying files unless the user explicitly upgrades the mode or asks it to proceed
- estimate likely affected areas
- identify risks and dependencies
- identify required tools or integrations

## 4.3 Goal

Goal mode is outcome-oriented.

The user provides an objective such as:
- build this app
- fix all failing tests
- migrate this project
- add multiplayer
- create a release
- prepare a pull request

Conductor should:
- clarify the goal
- build a task graph
- assign work
- coordinate agents
- verify results
- continue until the completion criteria are met or a real blocker requires the user

Goal mode should not stop just because one model finished a subtask.

Goal mode is responsible for orchestration.

## 4.4 Agent

Agent mode grants tool use according to policy.

Potential capabilities:
- read files
- write files
- create files
- delete files
- rename files
- run shell commands
- run tests
- launch applications
- control browser sessions
- use computer controls
- inspect Git
- create branches
- commit
- open PRs
- use GitHub
- install tools
- install MCP servers
- install skills
- install plugins
- manage tunnels
- operate remote environments

---

# 5. Clarifying-question system

Conductor must have a high-quality clarification system.

For ordinary prompts:
- ask at most 7 high-value questions in a run
- prefer fewer than 7
- ask zero questions if the task is already sufficiently specified
- never waste a question on something Conductor can safely detect or infer
- never ask for information already available from the project or user settings

Questions should be prioritized internally:

1. blocking decision
2. important product or design choice
3. optional preference

Normally only categories 1 and 2 should be surfaced.

Examples of good questions:
- Which engine do you want: HTML/Three.js, Godot, Unity, or Unreal?
- Is this web-only or native desktop?
- Is multiplayer required?
- Which target platforms matter?
- Do you want realistic or stylized visuals?
- Is this intended for low-end hardware?
- Should deployment be local, self-hosted, or cloud?

Examples of questions that should usually not be asked:
- May I install a normal dependency when Full Access is enabled?
- May I create a project folder?
- May I run tests?
- May I inspect the repository?
- May I install a required MCP that the user already told Conductor to install?

Add a visible:
- “Decide for me” option

If selected, Conductor chooses a sensible default.

For Goal mode:
- each clarification round may contain up to 7 questions
- Goal mode may ask another clarification round later only when genuinely necessary
- the 7-question limit applies per clarification round
- Goal mode can continue asking later until the goal is sufficiently specified or completed
- avoid repetitive questioning

Questions should be grouped cleanly in the UI rather than appearing as seven separate interruptions.

---

# 6. Single-model and Combo selection

The user must be able to choose either:

- one model
- a Combo

Both should use the same simple picker.

Do not force users into multi-model orchestration.

A single-model workflow must feel first-class.

The main composer area should make model selection easy.

The user should not need to click through many provider pages just to choose a model.

---

# 7. Combos

Combos are a core Conductor feature.

A Combo is a reusable orchestration configuration.

A Combo may contain:
- one provider with multiple models
- multiple providers
- multiple models from the same provider
- roles
- effort levels
- routing rules
- usage rules
- fallback rules
- review rules
- tool permissions
- budget limits
- provider reserve rules
- concurrency rules

Example:
- Gemini model for planning/research
- Codex model for implementation
- Claude model for review

Another valid Combo:
- Gemini Pro for architecture
- Gemini Flash for cheap sub-tasks
- no other provider

Combos must support:
- create
- edit
- duplicate
- rename
- export
- import
- share
- version
- enable/disable individual members
- default per project
- default globally

Ship useful built-in presets such as:
- Balanced
- Fast
- Low Usage
- Maximum Quality
- Coding
- Research
- UI / Design
- Local + Cloud

Do not hardcode these presets to current model names forever.

Preset logic should adapt to the models and providers actually available.

---

# 8. Provider-aware routing

Conductor should route work based on actual capabilities and user preferences.

Routing inputs may include:
- provider availability
- user subscription
- API key availability
- model capability
- rate limits
- model context size
- current usage
- estimated remaining usage
- task type
- task difficulty
- model latency
- configured budget
- user preference
- reserve policy
- previous failures
- provider outage
- tool compatibility

Do not assume every provider exposes exact remaining quota.

If exact quota is not available:
- estimate carefully
- label it as estimated
- never pretend it is exact

The router should favor the user’s strongest available subscription when that is beneficial.

But it must respect user configuration.

Example strategies:
- Balanced
- Use OpenAI first
- Use Claude first
- Use Gemini first
- Preserve Claude
- Preserve OpenAI
- Lowest API cost
- Fastest
- Maximum quality
- Subscription-first
- Manual weights

---

# 9. Usage Reserve

Add Usage Reserve rules.

Examples:
- preserve 30% of Claude capacity for review
- let Gemini handle most research
- prioritize Codex for implementation
- avoid API billing until subscription usage is exhausted
- reserve highest-effort usage for difficult work

These should be configurable globally and per Combo.

---

# 10. Effort / reasoning controls

Model effort must be capability-driven.

Do not invent effort levels.

For every model:
- detect supported reasoning / effort controls
- expose only valid levels
- map provider-specific controls into a simple Conductor abstraction where practical
- preserve access to advanced provider-specific controls where useful

The user interface should use a clean control similar to a slider or compact selector.

Do not force a long list of toggles.

Automatic effort selection is allowed.

However:

**Conductor must not automatically use Max / Ultra / highest effort unless the user has explicitly allowed that behavior.**

If Conductor believes a higher effort level would materially improve the result:
- ask the user
- explain briefly why
- offer an option such as “Allow”
- offer “Always allow for this model”
- offer “Don’t ask again”
- allow the choice to be reset in Settings

Automatic effort should normally follow a conservative escalation path:

Low → Medium → High → alternate model → multi-model review

Escalate only when warranted.

Examples:
- rename: low
- trivial syntax fix: low
- normal feature: medium
- architecture decision: high
- difficult debugging: high
- repeated failed approach: escalate
- security-sensitive change: consider high and review

The goal is to avoid wasting premium usage.

---

# 11. Provider capability cards

Each provider/model should expose a capability record.

Examples:
- coding
- reasoning
- vision
- image input
- image output
- browser tools
- computer use
- function calling
- MCP
- context size
- structured output
- file tools
- audio
- streaming
- reasoning controls
- supported effort levels
- max output
- known limitations

Conductor UI should generate relevant controls from these capabilities.

Do not redesign the whole app every time a provider releases a new model.

---

# 12. Provider catalog updates

Separate:
- Conductor application updates
- provider catalog updates

A new model should not necessarily require an app release.

Provider catalog refresh should be able to update:
- available models
- capability metadata
- deprecated models
- aliases
- reasoning settings
- context limits
- compatibility notes

Catalog data should be signed, verified, or fetched from trusted provider sources where appropriate.

---

# 13. Authentication

Providers should support the authentication methods they officially and technically allow.

Primary target:
- OAuth where supported
- API key where supported

Authentication must be production-grade.

Credentials must never be casually stored in plaintext.

Use OS credential storage when available:
- Windows Credential Manager
- macOS Keychain
- Linux Secret Service or suitable secure equivalent

Refreshable sessions should remain active when possible.

If a session expires:
- preserve project state
- preserve Goal state
- preserve agent state
- clearly say the provider has been signed out
- provide a direct “Sign in” action
- resume interrupted work after authentication when possible

Example message:
“Signed out — reconnect to continue Claude work.”

OAuth flows must be genuinely tested end-to-end during implementation.

Do not claim OAuth works unless it has been tested successfully.

Do not fake provider login success.

Do not implement unsupported or unofficial credential extraction.

---

# 14. Conductor account model

Conductor itself should not require a user account.

The app should work locally after installation.

Do not introduce mandatory cloud registration.

Remote access should not depend on a central Conductor account.

---

# 15. Local-first storage

Store locally by default:
- app settings
- project settings
- provider configuration metadata
- model preferences
- Combos
- instructions
- agent roles
- routing policies
- context indexes
- cached summaries
- MCP config
- plugin config
- skill config
- history
- checkpoints
- UI theme selection

Secrets should use secure OS storage.

Avoid uploading user projects to a Conductor-controlled server.

---

# 16. Context system

The context system is one of the most important parts of Conductor.

The objective is:

**Use the minimum context necessary to preserve high-quality output.**

Auto compression is enabled by default.

Conductor should optimize context before optimizing model selection.

Use this priority order:

1. current task
2. changed files
3. directly related symbols and files
4. project rules
5. recent decisions
6. older history only when needed

Do not repeatedly send:
- unchanged files
- full repository trees
- full logs
- entire chat histories
- old irrelevant decisions
- duplicate tool output
- repeated documentation
- unchanged system context

---

# 17. Smart Context Cache

Build a Smart Context Cache.

Cache:
- parsed files
- symbols
- references
- imports
- dependency relationships
- Git state
- changed-file state
- project structure
- summaries
- tool results
- test summaries
- recent decisions
- provider-independent handoff packets

Cache validity must be tied to content hashes or equivalent versioning.

If a file changes:
- invalidate only affected cache entries where practical
- recompute incrementally
- do not throw away the entire index unnecessarily

Cache must be safe and deterministic.

Cache should work across app restarts when useful.

---

# 18. Context compression pipeline

Use a multi-stage pipeline.

Suggested order:

1. repository filtering
2. file relevance scoring
3. symbol relevance scoring
4. changed-file priority
5. duplicate removal
6. stale-context removal
7. conversation compaction
8. build/log reduction
9. MCP description compression
10. provider prompt construction
11. output compression where appropriate
12. compact handoff construction

Compression must never silently remove required constraints.

Preserve:
- user requirements
- project invariants
- important decisions
- security boundaries
- acceptance criteria
- unresolved blockers

Advanced settings should allow inspection of what is being included.

---

# 19. Context Budget indicator

Add a small Context Budget indicator near the prompt.

It should communicate approximately:
- context size
- estimated token load
- whether compression is active
- whether cache is being reused

Keep it visually simple.

Advanced users may inspect:
- selected files
- selected symbols
- why they were selected
- estimated tokens
- compressed history size
- cached data reuse

---

# 20. Context Inspector

Provide an advanced Context Inspector.

It should show:
- what will be sent
- what was omitted
- what was summarized
- why a file or symbol was selected
- approximate token impact
- provider-specific final context

Do not require normal users to use it.

---

# 21. Token efficiency

Track useful efficiency metrics.

Examples:
- estimated tokens sent
- cached context reused
- duplicate context avoided
- context compression ratio
- log reduction
- repository data filtered locally
- estimated tokens avoided
- provider usage
- handoff size

Do not fabricate exact savings.

Label estimated values as estimates.

---

# 22. Caveman integration

Conductor must integrate a plugin called **Caveman** as an optimization component.

Requirements:
- included / installed by default where licensing and packaging allow
- enabled by default
- user can disable it in Settings
- updates automatically
- update source must be verified before implementation
- verify license compatibility before redistribution
- verify checksums/signatures where available
- preserve previous known-good version
- rollback on bad update
- do not hot-swap it unsafely in the middle of an active Goal
- safe updates may download immediately and activate at a safe boundary
- if incompatible with a provider, disable only the affected integration instead of breaking the entire app

Settings location could be:
Settings → Optimization → Caveman

Do not promise a fixed token reduction percentage.

Show upstream claims only if clearly attributed and verified.

Conductor must still have its own context optimization pipeline and must not depend entirely on Caveman.

---

# 23. Local optimization engine

Conductor may use a small local helper/optimization model where useful, but do not present a local LLaMA model as one of the primary built-in cloud providers.

The helper may perform cheap local tasks such as:
- relevance scoring
- summarization
- classification
- routing hints
- log reduction
- duplicate detection
- context compression

The app must still function if the local helper is unavailable.

Deterministic Rust fallbacks should exist for:
- file indexing
- symbol maps
- token estimation
- relevance heuristics
- Git inspection
- log trimming
- dependency discovery
- change tracking

If an optional helper model is downloaded:
- do not block startup
- show progress in the background
- support low-resource systems
- make it removable
- verify source and checksum

---

# 24. Custom providers

Users must be able to add custom providers.

Support provider adapters for:
- OpenAI-compatible APIs
- Anthropic-compatible APIs where sensible
- local providers
- Ollama-like endpoints
- LM Studio-like endpoints
- custom HTTP APIs
- future extensible provider definitions

Custom providers may use:
- API keys
- base URLs
- custom headers
- model lists
- context limits
- capability declarations
- effort mappings

Never assume custom providers are safe.

Show clear trust/permission boundaries.

---

# 25. Agent roles

Support reusable roles.

Examples:
- Architect
- Planner
- Coder
- Reviewer
- Researcher
- Debugger
- Tester
- Release Manager
- Documentation
- Security Reviewer

Roles are task-level concepts.

A provider is not permanently assigned to one role.

Roles can move between providers during a Goal.

---

# 26. Dynamic provider roles

If one provider:
- runs out of usage
- is rate-limited
- goes offline
- fails repeatedly
- lacks a needed tool

Conductor may transfer the role to another suitable model.

Example:
Claude was Reviewer.
Codex quota runs out.
Claude temporarily becomes Implementer.

The role belongs to the task, not the provider.

---

# 27. Handoff packets

Create a compact provider-independent handoff format.

A handoff packet should contain only what the next model needs.

Include:
- goal
- current subtask
- completed work
- important decisions
- changed files
- relevant symbols
- Git state
- tests run
- failures
- constraints
- unresolved blockers
- next steps
- tool state if relevant

Do not transfer giant chat transcripts by default.

This is critical for token efficiency.

---

# 28. Duplicate-work prevention

Conductor should detect when work has already been performed.

If one model researched a topic:
- reuse that result
- do not automatically ask another model to redo it

Allow independent review when intentionally requested.

Differentiate:
- redundant duplicate work
- independent validation

---

# 29. Multi-model review

For important tasks:
- one model may implement
- another may review
- a third may resolve disagreements

Review should focus on:
- correctness
- requirements
- tests
- security
- regressions
- maintainability

Do not require multi-model review for trivial tasks.

---

# 30. Agent disagreement UI

When agents disagree, do not dump a huge argument into chat.

Summarize clearly:

- Option A
- consequences
- evidence
- Option B
- consequences
- evidence
- shared agreement
- unresolved decision

Let the user decide when the decision is product-significant.

---

# 31. Task graph

Goal mode should maintain a task graph.

Example:
Plan → Research → Backend → UI → Tests → Review → Fixes → Complete

A task node may show:
- status
- assigned role
- assigned model
- elapsed time
- files touched
- tool state
- verification state
- usage estimate

Do not overload the default UI.

Use an expandable detail view.

---

# 32. Goal Contract

When Goal mode begins, create a Goal Contract.

It should contain:
- objective
- requirements
- constraints
- target platforms
- acceptance criteria
- Definition of Done
- budget policy
- permissions
- important decisions

The user can edit it.

Agents must treat it as authoritative.

---

# 33. Definition of Done

Support explicit completion checks.

Examples:
- cargo test passes
- cargo clippy passes
- formatting passes
- Windows build passes
- Linux build passes
- macOS build passes
- UI screenshot matches acceptance criteria
- project launches
- no known critical errors
- requested documentation exists

Do not mark a Goal complete merely because an agent says it is complete.

---

# 34. Test Gate

Conductor should support project-configured verification.

Examples:
- cargo test
- cargo fmt --check
- cargo clippy
- npm test
- npm run build
- pytest
- gradle test
- mvn test
- CMake build
- Godot validation
- custom commands

A Goal may require tests before completion.

---

# 35. Visual testing

For visual applications:
- launch the app
- capture screenshots
- test common resolutions
- test scaling
- inspect layout regressions
- allow a reviewer model to evaluate screenshots

Support user feedback tied to:
- screenshot regions
- UI elements
- visible text
- app state

---

# 36. Computer use

Computer use must be first-class.

Depending on permissions, agents may:
- inspect screen
- click
- type
- launch apps
- use browser
- use IDEs
- use terminals
- interact with graphical tools
- control one app or the full desktop

Support scope restrictions.

Example:
- browser only
- terminal only
- one window only
- one application only
- project workspace only
- full desktop

---

# 37. Permission levels

Use simple names:

- Ask
- Auto Approve
- Full Access

## Ask
Request approval for actions outside safe read-only behavior.

## Auto Approve
Automatically approve actions within configured categories.

## Full Access
Allow broad execution without repeated prompts.

Full Access must:
- be explicit
- be visually obvious
- be revocable instantly
- not disable download integrity checks
- not disable security boundaries silently
- not bypass OS security mechanisms
- not expose secrets unnecessarily

Full Access means fewer prompts, not reckless execution.

---

# 38. Fast Stop

Provide an obvious Stop control.

Stopping should be able to cancel:
- agent actions
- pending tool calls
- terminal jobs
- browser automation
- computer-use actions
- queued sub-tasks

Where immediate termination is unsafe, clearly show the state.

---

# 39. Emergency Stop

Provide a global emergency stop shortcut.

This should halt active autonomous actions as quickly as safely possible.

Especially important for:
- Full Access
- computer use
- remote control
- package installation
- terminal execution

---

# 40. Background service

Conductor should continue active work when the main UI is closed.

Implement a lightweight background process/service.

Requirements:
- continue active Goals
- continue approved work
- preserve state
- survive UI restart
- expose pause/resume/stop controls
- support tray/menu-bar integration where appropriate
- use very low resources when idle

Closing the window must not necessarily terminate active work.

The user must be able to choose whether closing the app:
- minimizes to background
- exits completely
- asks each time

---

# 41. Session resume

When reopening:
- restore last project
- restore active Goal
- restore chat
- restore task graph
- restore model assignments
- restore pending questions
- restore terminal sessions where technically possible
- restore browser state where technically possible
- restore unfinished work

Crash recovery should be supported.

---

# 42. History

Include a normal feature called **History**.

Do not call it “Time Machine”.

History should show meaningful activity:
- tasks
- decisions
- file changes
- checkpoints
- model handoffs
- verification results

Avoid storing unnecessary noise.

---

# 43. Checkpoints

Before major risky phases, create lightweight Checkpoints.

Examples:
- before dependency upgrade
- before authentication rewrite
- before major refactor
- before migration
- before destructive file operation

Checkpoints should allow restoration of relevant project state.

Use Git intelligently when available.

Do not duplicate huge repositories unnecessarily.

---

# 44. File ownership and concurrent work

If multiple agents work at once:
- track which task owns which file/component
- avoid simultaneous uncontrolled edits
- detect conflicts early
- prevent silent overwrites

Use Git worktrees or equivalent isolation where appropriate.

---

# 45. Git worktree strategy

For parallel coding:
- prefer isolated branches/worktrees
- merge after verification
- detect conflicts
- never discard unrelated user changes
- never overwrite uncommitted user work

A coordinator can review and merge results.

---

# 46. Conflict resolution

When two agents edit the same area:
- detect the conflict
- show both versions
- preserve both
- optionally ask a third model to produce a candidate merge
- verify the result

Do not silently pick one.

---

# 47. Git integration

Support:
- status
- diff
- branches
- commits
- worktrees
- stash
- logs
- blame
- merge
- conflict inspection

Respect existing user changes.

Never reset or discard user work without explicit permission.

---

# 48. GitHub integration

Support deep GitHub workflows.

Potential capabilities:
- repositories
- issues
- branches
- pull requests
- reviews
- comments
- Actions
- releases
- code search
- commit status
- CI failures
- review-comment fixes

Workflow example:
Issue → plan → implementation → tests → diff review → branch → commit → PR → CI → review comments → fixes

Authentication must be secure.

---

# 49. Browser

Conductor should include an embedded or integrated browser experience.

Capabilities may include:
- normal browsing
- DOM inspection
- screenshots
- console logs
- network logs
- downloads
- browser profiles
- cookies
- authenticated sessions
- user login handoff
- project-specific browser isolation

Browser sessions should be reusable when safe.

Do not spawn excessive browser processes.

---

# 50. Remote access

Conductor should support remote access without requiring a central Conductor account.

Use a local **Conductor Host** process on the machine containing the project.

The host may expose:
- project files
- file changes
- chat state
- task state
- terminal output
- agent activity
- permission prompts
- Goal progress

Remote access works only while the host machine is on and reachable.

Use:
- encrypted authenticated connections
- explicit device trust
- revocable sessions
- scoped project access

Do not require uploading the repository to a central Conductor server.

---

# 51. Real-time file synchronization

For remote sessions:
- watch project files
- transmit diffs where practical
- use file hashes/version numbers
- detect conflicts
- avoid full-file retransmission when not needed
- preserve correctness over bandwidth savings

Changes made locally should appear remotely quickly.

Changes made by remote agents should appear locally quickly.

---

# 52. Real-time state channel

Maintain a lightweight real-time state channel for:
- chat
- task status
- permission requests
- terminal output
- model status
- token estimates
- file changes
- Goal progress
- provider auth state

The UI should update nearly instantly.

---

# 53. Disconnect-safe Goal execution

If a remote client disconnects:
- the host may continue approved Goal work
- stop only if policy requires user presence
- if a product decision is required, pause at that decision
- preserve state
- show current progress after reconnect

---

# 54. MCP

MCP must be a major feature, not an advanced afterthought.

Conductor should make MCP easier than existing developer workflows.

Users should be able to say:
- “Install this MCP”
- “Set up Blender MCP”
- “Add this GitHub MCP”
- “Connect this MCP server”
- “Fix this broken MCP”

Conductor should:
- discover requirements
- locate the correct integration
- verify source
- install dependencies
- configure it
- register it
- test it
- expose it to compatible agents
- report success or actionable failure

With Full Access, do not repeatedly ask permission for every install step.

Still ask product decisions if necessary.

---

# 55. MCP Doctor

Create an MCP Doctor.

Show:
- installed
- running
- connected
- authenticated
- outdated
- broken
- missing dependency
- wrong configuration
- permission issue

Include:
- Fix
- Reconnect
- Update
- Disable
- Remove

Keep UI simple.

---

# 56. Shared MCP layer

Where technically possible, configure an MCP once and expose it across:
- Codex
- Claude
- Gemini
- custom compatible providers

Use adapters.

Avoid forcing users to configure the same MCP three times.

---

# 57. Skills

Create a universal Skills layer.

A skill may include:
- instructions
- tool definitions
- commands
- scripts
- schemas
- MCP dependencies
- provider-specific adapters
- permissions
- version metadata

Install once in Conductor.

Expose compatible functionality to supported providers.

---

# 58. Skill installation

Users should be able to say:
- “Install this skill”
- “Install the Steam skill”
- “Add this GitHub skill”
- “Update my skills”

Conductor should:
- verify source
- inspect permissions
- install
- configure
- test
- update
- remove
- rollback when possible

---

# 59. Plugins

Support plugins as a broader extension mechanism.

Plugins must have:
- manifests
- explicit permissions
- source information
- versioning
- update policy
- isolation where practical
- removal
- health state

Do not let arbitrary plugins silently gain unrestricted access.

---

# 60. Community content

For now, community sharing should focus on:
- Skills
- UI themes

Do not build a broad community-template marketplace yet.

Do not add Quiet Mode as a product feature for now.

---

# 61. UI themes

Users should be able to:
- select themes
- install themes
- create themes
- share themes
- update themes
- disable themes
- remove themes

Theme customization may cover:
- colors
- typography
- spacing
- surfaces
- editor appearance
- borders
- icons
- subtle animation curves
- agent status visuals

Theme extensions must be sandboxed or constrained enough to avoid unsafe arbitrary execution.

The future public GitHub README should include:
- theme format
- starter template
- theme creation instructions
- preview workflow
- packaging
- installation
- versioning

---

# 62. Custom agent instructions

Support:
- global user instructions
- provider-specific instructions
- project instructions
- role instructions
- Combo instructions

Store locally.

Clearly show precedence.

Suggested precedence:
1. safety/system invariants
2. project Goal Contract
3. project instructions
4. role instructions
5. provider-specific instructions
6. user global instructions

Avoid duplicated prompt bloat.

Compile only relevant instructions into each request.

---

# 63. Decision memory

When the user makes a project decision:
- remember it locally
- do not ask again unnecessarily

Examples:
- use Godot
- target web
- PostgreSQL
- do not use Docker
- support Windows 10

Allow users to inspect and edit project decisions.

---

# 64. Project Memory

Project Memory should store durable project facts.

Do not dump entire conversations into memory.

Good memory:
- architecture decisions
- supported platforms
- dependency policy
- naming rules
- deployment decisions
- API invariants
- user constraints

Bad memory:
- every casual sentence
- repeated logs
- transient tool output

---

# 65. Environment Doctor

Build an Environment Doctor.

Detect relevant tooling:
- Rust
- Cargo
- Git
- Node
- npm/pnpm/yarn
- Python
- Java
- Gradle
- Maven
- Docker
- WSL
- compilers
- CMake
- SDKs
- GPU drivers where relevant
- Godot
- Unity
- Unreal
- Blender
- project-specific tools

Detect:
- missing PATH
- wrong version
- permission issue
- broken install
- missing dependency

With sufficient permission, offer:
- Fix
- Install
- Reconfigure

---

# 66. Tool discovery

If an agent realizes a missing tool would materially improve the work:
- propose or install it according to permissions

Example:
A 3D asset task may benefit from Blender.

Full Access:
- Conductor may install and configure it automatically if technically safe and unambiguous

Ask / Auto Approve:
- follow policy

Still ask if there is a meaningful product choice.

---

# 67. Project profiles

Support project profiles.

Profiles may define:
- default Combo
- tools
- MCPs
- skills
- permissions
- instructions
- environment variables
- ignore rules
- startup commands
- test commands
- target platforms
- performance mode

---

# 68. Project auto-detection

Detect common project types:
- Cargo
- npm
- pnpm
- yarn
- Gradle
- Maven
- Python
- CMake
- Godot
- Unity
- Unreal
- Docker
- monorepos
- Git submodules

Use detection to improve setup.

Do not assume.

---

# 69. Project setup recipes

Support reusable setup recipes.

Examples:
- Rust Desktop App
- Three.js Game
- Godot Web Game
- Paper Plugin
- React + Rust Backend
- Unity Game

Recipes may include:
- dependencies
- agents
- skills
- MCPs
- test commands
- recommended Combo
- environment checks

---

# 70. Project config file

Support a project configuration file, for example:

conductor.toml

It may contain:
- project metadata
- default Combo
- preferred providers
- test commands
- supported platforms
- MCPs
- skills
- permissions
- instructions references
- context rules
- ignore rules
- environment requirements

Never store raw secrets in project config.

---

# 71. Preview Center

Provide integrated previews where practical:
- websites
- games
- desktop apps
- images
- PDFs
- documents
- generated assets

Do not require the user to constantly switch windows.

---

# 72. Visual feedback

Allow the user to point at a preview and say:
- move this
- change that button
- this area looks wrong
- use this spacing

Pass the relevant visual context to the agent.

---

# 73. Tunnels

Support development tunnels and preview sharing.

Capabilities:
- local port detection
- dev server tracking
- local preview
- temporary remote link where supported
- tunnel lifecycle
- close on task completion
- explicit permission policies

Avoid leaving tunnels running accidentally.

---

# 74. Secrets

Secrets must be protected.

Whenever possible:
- inject a secret into an approved process
- avoid exposing the raw value to the model
- redact from logs
- redact from context
- redact from history

---

# 75. Secret scanner

Before sending context to a cloud provider, scan for likely:
- API keys
- tokens
- private keys
- .env values
- SSH keys
- passwords
- credentials

Redact when not required.

Allow explicit user overrides.

---

# 76. Privacy profiles

Support profiles such as:
- Local First
- Standard
- Restricted

Restricted may:
- block sensitive directories
- require explicit approval for cloud transmission
- disable some remote capabilities
- redact more aggressively

---

# 77. Large repository mode

Support large monorepos.

Use:
- incremental indexing
- dependency graphs
- symbol graphs
- changed-file prioritization
- lazy parsing
- targeted loading
- local search

Do not send a whole monorepo to a model.

---

# 78. Build/log reduction

When a build log is huge:
- identify the actual failure
- preserve preceding context
- preserve stack trace
- preserve relevant warnings
- remove repeated noise
- link to full log locally

Do not send 30,000 lines when 200 lines are sufficient.

---

# 79. Automatic retry strategy

Do not blindly retry the same failed call.

Possible recovery:
- reduce context
- repair tool
- change effort
- switch provider
- switch model
- lower concurrency
- rerun only failed test
- refresh auth
- ask user only when truly necessary

---

# 80. Provider health

Provide a simple provider health view.

Possible states:
- connected
- signed out
- rate-limited
- unavailable
- degraded
- unknown
- API key invalid
- model missing

Do not spam notifications.

---

# 81. Credential repair

If auth breaks:
- explain simply
- offer reconnect
- preserve work
- resume after reconnect

Do not show raw auth internals unless expanded.

---

# 82. Notifications

Notify for meaningful events:
- user decision required
- permission required
- Goal complete
- provider signed out
- provider usage exhausted
- unrecoverable test failure
- critical update issue
- remote session request

Do not notify for every tool call.

---

# 83. App updates

Conductor must support self-update.

Updates should be:
- signed
- verified
- rollback-capable
- resumable
- transparent

Show update progress.

Preserve previous working version until the update is confirmed healthy.

---

# 84. Update channels

Consider:
- Stable
- Beta
- Nightly

Do not force unstable channels.

---

# 85. Integration updates

Separate update mechanisms for:
- Conductor app
- provider catalogs
- plugins
- skills
- Caveman
- themes
- optional helper components

---

# 86. Installation receipts

When Conductor installs something:
record:
- package/tool name
- source
- version
- install location
- checksum/signature if available
- install date
- project/global scope
- removal method

---

# 87. Clean removal

Conductor should know how to remove tools it installed.

Support:
- remove plugin
- remove skill
- remove MCP
- remove helper model
- remove temporary tool
- clean project-only install

Never remove unrelated user-installed software accidentally.

---

# 88. Temporary tools

Allow project-scoped or Goal-scoped tools.

After completion:
- offer removal
- keep if user wants
- record decision

---

# 89. Custom setup wizard

Conductor must have a custom, branded setup/onboarding wizard.

This is a hard product requirement.

The wizard must:
- have a custom Conductor logo
- look professional
- match the main application theme
- be simple
- use calm spacing
- use clean typography
- avoid default installer/toolkit appearance
- avoid raw programmer UI
- avoid a generic generated dashboard look

The setup experience should feel like part of the same product, not a separate utility.

Suggested flow:
1. Welcome / logo
2. performance profile
3. provider connection
4. single-model or Combo preference
5. permissions preference
6. optional remote access setup
7. optimization setup
8. ready

Do not force every step if unnecessary.

Allow skip and configure later.

---

# 90. Visual design quality

The UI must be designed, not merely “coded until functional”.

Use a coherent design system.

Define:
- typography scale
- spacing scale
- corner radius
- surface hierarchy
- border treatment
- icon style
- motion rules
- dark/light theme behavior
- focus states
- keyboard states
- loading states
- empty states
- error states

Use a custom logo asset.

Do not ship placeholder branding.

Do not ship default framework styling.

---

# 91. Main layout

Keep the main layout simple.

Suggested structure:

Left:
- project history
- conversations
- goals

Center:
- main conversation/work area
- task output
- previews when appropriate

Composer:
- prompt field
- single model / Combo selector
- effort control
- mode selector
- send / run
- Stop when active

Advanced:
- expandable panels
- Settings
- tool views
- context inspector
- provider health
- MCP Doctor

Avoid permanent clutter.

---

# 92. Animations

Use subtle animations only.

Good:
- activity indicator
- progress transitions
- small panel motion
- lightweight Conductor mark animation
- task completion transition

Bad:
- constant glowing
- floating AI orbs
- unnecessary particle effects
- distracting loops
- expensive visual effects

Animations must scale down in Potato mode.

---

# 93. Portable mode

Support a portable mode where practical.

Goal:
- run from a folder/portable installation
- avoid admin requirements when possible
- keep portable config separate
- clearly show where data is stored

---

# 94. Config export/import

Allow export/import of:
- Combos
- skills references
- MCP configs
- themes
- project recipes
- keybindings
- instructions
- routing policies
- non-secret provider settings

Do not export raw secrets by default.

---

# 95. Command palette

Add a keyboard-first command palette.

Possible actions:
- open project
- new project
- switch Combo
- choose model
- set effort
- start Goal
- open MCP Doctor
- run tests
- open terminal
- open browser
- pause Goal
- stop agents
- open Settings

Keep it fast.

---

# 96. Keyboard navigation

The app should be usable efficiently by keyboard.

Provide:
- logical tab order
- shortcuts
- command palette
- focus indicators
- configurable keybindings where practical

---

# 97. Accessibility

Support:
- readable contrast
- scalable UI
- screen-reader-friendly labels where supported
- keyboard navigation
- reduced motion
- font scaling

---

# 98. Adaptive Combo behavior

A Combo may change behavior by phase.

Example:
- architecture: high-quality planner
- implementation: efficient coder
- repetitive fixes: cheaper model
- review: strong reviewer

This should be explicit and inspectable.

---

# 99. Model replacement rules

If a model is deprecated:
- detect it
- identify compatible alternatives
- offer migration
- automatically migrate only if user policy permits
- preserve original config history

---

# 100. No provider lock-in

Internally, represent tasks in a provider-neutral format.

Avoid deeply coupling Goal state to one provider’s message schema.

A task started by one model should be transferable to another.

---

# 101. Background downloads

Optional components should download without blocking the main UI.

Examples:
- helper model
- Caveman updates
- provider catalogs
- themes
- skills
- plugins

Show:
- progress
- pause/cancel where useful
- source
- version

---

# 102. Smart defaults

Do not interrogate users about every technical choice.

If the choice is low-risk and reversible:
- pick a sensible default

If the choice fundamentally changes the product:
- ask

Example:
Choosing Godot vs Unity is important.
Choosing a normal minor dependency version is usually not.

---

# 103. User-facing language

Use clear language.

Prefer:
“Claude signed out. Reconnect to continue.”

Avoid:
“OAuth refresh_token grant failed with HTTP 401 invalid_grant.”

Advanced error details may exist behind disclosure.

---

# 104. Security model

Treat Conductor as a powerful local automation platform.

Threat model includes:
- malicious plugins
- malicious MCP servers
- prompt injection
- malicious repositories
- poisoned project files
- untrusted downloaded tools
- compromised provider responses
- accidental secret exposure
- remote session hijacking
- unsafe computer-use actions

Build explicit defenses.

---

# 105. Tool permission model

Every tool/integration should declare capabilities such as:
- filesystem.read
- filesystem.write
- filesystem.delete
- terminal.execute
- network
- browser
- computer.control
- github
- secrets.use
- tunnels
- install.software
- mcp
- remote.host

Use these in permission policy.

---

# 106. Prompt injection defense

When agents inspect:
- webpages
- repositories
- docs
- issues
- comments
- external files

Treat embedded instructions as untrusted content unless explicitly intended as instructions.

Separate:
- user instruction
- system policy
- project config
- retrieved content

Do not let retrieved text silently override higher-priority instructions.

---

# 107. Downloads and integrity

Before running downloaded binaries/scripts:
- verify source
- verify checksum/signature where available
- inspect metadata
- avoid unsafe pipe-to-shell patterns
- preserve provenance
- record install receipt

Full Access does not disable this.

---

# 108. Remote security

Remote access should use:
- strong authentication
- encryption
- short-lived pairing or trust establishment
- revocation
- device list
- scoped project access
- session audit

Do not expose arbitrary remote control by default.

---

# 109. Logging

Logs should be:
- structured
- useful
- privacy-aware
- redact secrets
- rotatable
- bounded in size

Provide diagnostic export.

---

# 110. Telemetry

Because Conductor is open source and local-first:
- telemetry must be optional
- default to privacy-respecting behavior
- clearly disclose collected fields
- never send project source code
- never send prompts without explicit consent

The app must work fully without telemetry.

---

# 111. Open-source quality

Repository should include:
- README
- LICENSE
- CONTRIBUTING
- SECURITY
- CODE_OF_CONDUCT
- CHANGELOG
- ROADMAP
- build instructions
- architecture docs
- provider adapter docs
- MCP docs
- skill docs
- theme docs
- release docs
- security model
- privacy docs

Use an OSI-compatible license chosen deliberately.

Do not accidentally bundle dependencies with incompatible licensing.

---

# 112. Rust architecture

The application should be primarily Rust.

Use Rust for:
- orchestration engine
- provider abstraction
- auth coordination
- context engine
- file indexing
- Git integration
- background service
- remote host
- task graph
- state management
- update system
- plugin/skill management
- MCP coordination
- security-critical code
- performance-critical code

UI technology may use another frontend layer if that produces a materially more professional cross-platform UI, but:
- Rust remains the core
- architecture must stay maintainable
- UI must not become a giant unstructured web app
- avoid unnecessary runtime bloat

Choose the desktop UI stack based on:
- startup speed
- memory
- cross-platform maturity
- accessibility
- visual polish
- installer quality
- maintainability
- update support

Document the choice.

---

# 113. Suggested crate boundaries

A possible workspace:

- conductor-app
- conductor-core
- conductor-providers
- conductor-auth
- conductor-context
- conductor-agents
- conductor-goals
- conductor-tools
- conductor-mcp
- conductor-skills
- conductor-plugins
- conductor-git
- conductor-github
- conductor-browser
- conductor-computer
- conductor-remote
- conductor-background
- conductor-updater
- conductor-config
- conductor-security
- conductor-telemetry
- conductor-ui-bridge
- conductor-cli
- conductor-testkit

Do not create crates merely for aesthetics.

Keep boundaries meaningful.

---

# 114. Provider adapter interface

Define a provider-neutral adapter interface.

It should support concepts such as:
- authenticate
- refresh auth
- list models
- get capabilities
- start request
- stream response
- tool calls
- usage metadata
- cancellation
- errors
- model availability
- effort controls
- context limits

Avoid leaking provider-specific structures into core orchestration.

---

# 115. Tool abstraction

Agents should use a unified tool interface.

Tools may include:
- files
- terminal
- browser
- computer
- Git
- GitHub
- MCP
- tunnel
- installer
- environment doctor
- preview

Every tool call should have:
- identity
- input
- permissions
- result
- logs
- cancellation state
- provenance

---

# 116. State persistence

Persist durable state carefully.

Examples:
- projects
- chats
- goals
- tasks
- decisions
- checkpoints
- Combos
- provider settings
- cache metadata
- installed integrations
- permissions
- remote trust

Use schema versioning.

Support migrations.

---

# 117. Crash safety

Use:
- atomic writes
- journaling or transactional persistence where appropriate
- corruption detection
- safe recovery
- backups of critical config

Do not corrupt project metadata after a crash.

---

# 118. Cross-platform installer

Create professional installers for supported OSes.

Requirements:
- signed releases where infrastructure permits
- clear install path
- custom Conductor logo
- polished setup wizard where the OS/package format allows
- predictable uninstall
- preserve user data unless explicitly removed
- secure update registration

Where package managers are used, preserve the same product identity.

---

# 119. Auto-start behavior

Background service auto-start should be configurable.

Default should be conservative.

The user should understand:
- whether Conductor runs at login
- whether background Goals continue
- whether remote host is enabled

---

# 120. No account requirement

Repeat this as a hard requirement:

**Do not require a Conductor account for normal use.**

Remote access must not require a central identity account.

Provider accounts remain provider-specific.

---

# 121. Community themes

Theme sharing can be Git-based.

Support:
- install from repository
- local theme folders
- version pinning
- update checks
- validation

README must explain how to build a theme.

---

# 122. Community skills

Skill sharing can also be Git-based.

Support:
- manifest
- permissions
- compatibility
- version
- source
- update policy
- optional signature

Avoid centralized lock-in.

---

# 123. No Quiet Mode for now

Do not implement Quiet Mode as a product feature in the initial scope.

The default interface itself should already be calm.

If advanced activity detail exists:
- make it expandable
- do not create a separate Quiet Mode concept

---

# 124. Automatic model effort

Default automatic effort should prefer efficiency.

Do not use maximum effort just because it exists.

Use evidence-based escalation.

If user has prohibited max effort:
- never escalate to max
- choose another strategy

If user allows max after prompt:
- remember the preference according to scope
- allow reset in Settings

---

# 125. Model picker UX

The model/Combo picker should be simple.

Suggested:
- one button/picker
- recent items
- favorites
- providers grouped cleanly
- Combos first if used often
- single models visible
- search

Next to it:
- effort control

Avoid dozens of tiny provider switches.

---

# 126. Background resource rules

When idle:
- no constant polling if push/event APIs are available
- no unnecessary browser process
- no unnecessary local model loaded
- no excessive filesystem scans
- no busy loops
- back off provider checks
- batch low-priority maintenance

---

# 127. Provider health polling

Use adaptive intervals.

Do not hammer APIs.

Refresh immediately when needed.

Use cached status otherwise.

---

# 128. Remote host resource rules

Remote host should be lightweight.

When no client is connected:
- minimize CPU
- minimize network
- watch only required files
- suspend expensive indexing if not needed

---

# 129. App responsiveness

Never block the UI thread on:
- provider requests
- Git operations
- indexing
- filesystem scans
- downloads
- builds
- browser automation
- update checks

Use asynchronous jobs and cancellation.

---

# 130. First-run experience

First launch should be usable quickly.

Suggested:
- show app immediately
- setup wizard
- connect at least one provider
- optionally select performance profile
- optionally install optimization helper
- create/open project

Do not require every provider.

---

# 131. Provider setup

Provider setup should be easy.

Show:
- Sign in with provider when supported
- Use API key
- test connection
- model availability
- auth state

After success:
- return to setup
- preserve progress

---

# 132. Auth testing

Build automated and manual auth test plans.

For OAuth:
- fresh login
- refresh
- expired token
- revoked access
- app restart
- background service
- multiple projects
- resume interrupted task
- logout
- reconnect

For API keys:
- valid
- invalid
- revoked
- network failure
- provider rate limit

Do not declare provider support production-ready before these pass.

---

# 133. Remote testing

Test:
- same LAN
- reconnect
- host restart
- client restart
- file conflict
- large file change
- terminal output
- permission prompt
- Goal continues after disconnect
- revoked device
- wrong credentials
- encrypted connection

---

# 134. Low-end hardware testing

Create a low-resource CI/manual profile.

Test:
- 8 GB RAM class machine
- limited CPU
- large repo
- one provider
- multiple providers configured but idle
- background service
- browser closed
- Potato mode

Track:
- startup time
- idle RAM
- idle CPU
- indexing latency
- prompt latency excluding provider network time

---

# 135. Startup profiling

Measure startup.

Break down:
- process start
- config load
- DB open
- UI first paint
- background service connection
- project restore
- provider catalog refresh
- indexing kickoff

Do not guess.

---

# 136. Benchmarks

Add benchmarks for:
- context selection
- context compression
- cache hits
- cache invalidation
- handoff packet size
- log reduction
- provider routing
- file indexing
- large repository scans
- remote diff sync

Do not fabricate benchmark results.

---

# 137. Security tests

Test:
- malicious MCP
- malicious plugin manifest
- prompt injection file
- secret in source
- symlink escape
- path traversal
- remote auth bypass
- stale session
- unsigned update
- tampered plugin
- tampered Caveman package
- malicious theme package
- command injection
- shell escaping

---

# 138. Update tests

Test:
- normal update
- interrupted download
- bad signature
- bad checksum
- failed install
- rollback
- downgrade policy
- provider catalog update
- plugin update
- Caveman update
- theme update
- background Goal during update

---

# 139. Multi-agent development rules for building Conductor

The implementation may itself be built by multiple coding agents.

If Codex and Claude Code or other agents work on the same repository, they must coordinate.

Rules:

1. Inspect repository state before editing.
2. Never assume another agent is idle.
3. Never overwrite unrelated uncommitted work.
4. Prefer clear ownership by subsystem.
5. Use separate branches/worktrees for parallel large changes.
6. Keep commits scoped.
7. Rebase/merge carefully.
8. Run relevant tests before handing off.
9. Document unfinished work.
10. Do not silently change architecture agreed elsewhere.
11. Use shared project docs for decisions.
12. Do not duplicate another agent’s assigned task.
13. If conflict exists, reconcile rather than discard.
14. Leave the repo in a buildable state whenever possible.

---

# 140. Coding quality

Rust code should:
- be idiomatic
- use strong types
- avoid unnecessary unsafe
- avoid panic in normal runtime paths
- have structured errors
- support cancellation
- have tests
- be documented where non-obvious
- keep modules focused
- use async thoughtfully
- avoid hidden blocking
- avoid needless clones/allocations in hot paths

---

# 141. Dependency policy

Prefer:
- mature crates
- permissive licenses
- actively maintained projects
- low dependency risk

Avoid:
- huge dependency trees without justification
- abandoned crates
- unnecessary native dependencies
- obscure binaries
- unverified installers

Document major dependency decisions.

---

# 142. UI implementation quality

The UI must not look like a generated prototype.

Before considering the UI done:
- establish design tokens
- use consistent spacing
- use custom icons or a coherent icon library
- use the Conductor logo
- polish hover/focus/disabled states
- polish setup wizard
- polish empty states
- polish loading states
- polish error states
- test high-DPI
- test scaling
- test dark/light mode
- test keyboard navigation
- test small laptop screens

---

# 143. Custom logo

Create a proper Conductor logo asset.

Requirements:
- simple
- memorable
- professional
- works small
- works monochrome
- works dark/light
- suitable for app icon
- suitable for installer
- suitable for GitHub
- suitable for tray/menu bar

Do not use placeholder icons.

Keep source assets in the repository.

---

# 144. Branding

Conductor branding should be restrained.

Avoid cliché AI imagery.

Do not overuse:
- brains
- robot heads
- magic sparkles
- neon gradients
- sci-fi control panels

The name itself communicates orchestration.

---

# 145. History sidebar

The left area may contain:
- recent projects
- recent chats
- recent Goals
- pinned items

Keep it compact.

Do not fill it with telemetry.

---

# 146. Project opening

Support:
- New Project
- Open Project

Open Project should use native file/folder selection where possible.

A project is generally a folder/repository.

Conductor should detect:
- Git
- project type
- language
- build system
- existing conductor.toml

---

# 147. New Project

New Project can:
- create empty folder
- initialize Git
- choose a recipe
- choose a framework
- use clarification wizard
- optionally install tools
- optionally initialize provider/MCP/skills

Do not force excessive setup.

---

# 148. Project Resume Summary

When opening an old project, optionally show a compact summary:
- last Goal
- branch
- unfinished tasks
- failing tests
- last major decision
- what likely needs attention

Do not show if nothing useful exists.

---

# 149. Resource Governor

Implement a resource governor.

Observe:
- CPU
- memory
- disk pressure
- active browser processes
- local helper usage
- indexing queue
- agent concurrency

Automatically reduce pressure.

---

# 150. Auto Potato detection

On first run, Conductor may perform a lightweight hardware assessment.

Recommend:
- Potato
- Balanced
- Maximum

Do not run an intrusive benchmark.

User can override.

---

# 151. Goal budgets

Goal mode may support:
- API cost limit
- token limit
- subscription preference
- provider usage reserve
- max parallel agents
- max runtime policy
- stop/ask threshold

Do not force budgets.

---

# 152. Usage exhaustion

If a model/provider runs out of allowed usage:
- state it clearly
- preserve the task
- transfer to another provider if allowed
- show which provider continues

Example:
“Claude usage unavailable. Continuing implementation with Codex.”

Do not restart from scratch.

---

# 153. Provider outage

If a provider fails:
- detect likely outage
- retry reasonably
- fail over when allowed
- preserve state
- do not spam repeated calls

---

# 154. Automatic reassignment

Reassignment should preserve:
- task
- context
- changed files
- tests
- blockers
- decisions

Use compact handoff packets.

---

# 155. Verification before completion

Before declaring a coding Goal complete:
- inspect diff
- run configured checks
- verify requested behavior
- review known failures
- confirm acceptance criteria

If unable to verify:
- say exactly what remains unverified

---

# 156. No fabricated success

Never claim:
- OAuth works
- remote works
- provider integration works
- tests pass
- update works
- cross-platform works
- benchmark target is achieved

unless there is actual evidence.

---

# 157. External requirements

If something requires:
- a provider account
- paid API access
- macOS signing
- Windows signing
- Apple notarization
- GitHub release secrets
- provider developer credentials

document it clearly.

Do not block local development unnecessarily.

---

# 158. CI

Set up CI for supported build/test matrices.

At minimum:
- Rust format
- Clippy
- tests
- build
- security/audit checks
- frontend checks if frontend tech exists
- packaging smoke tests where possible

Expand platform matrix as implementation matures.

---

# 159. Releases

Automate release artifacts.

Include:
- checksums
- signatures where available
- changelog
- platform packages
- SBOM where practical
- provenance/attestation where practical

---

# 160. Documentation

README should eventually include:
- what Conductor is
- screenshots
- install
- quick start
- providers
- Combos
- modes
- permissions
- remote access
- MCP
- skills
- themes
- updates
- privacy
- security
- contributing

Theme creation must have dedicated instructions.

---

# 161. Non-goals for initial release

Do not include these unless later explicitly added:

- mandatory Conductor cloud account
- broad community template marketplace
- Quiet Mode
- social network
- AI-generated public feed
- cryptocurrency/token system
- unnecessary cloud sync
- forced telemetry
- proprietary lock-in
- excessive gamification

---

# 162. Implementation order

Use phased implementation.

## Phase 0 — repository foundation
- workspace
- formatting
- linting
- CI
- docs skeleton
- config
- logging
- error model
- state store

## Phase 1 — desktop shell
- professional UI
- Conductor logo
- setup wizard
- project open/new
- settings
- background service connection
- session persistence

## Phase 2 — provider abstraction
- provider interface
- model catalog
- capability system
- API key auth
- OAuth foundations
- streaming
- cancellation

## Phase 3 — initial providers
- OpenAI/Codex
- Anthropic/Claude
- Google/Gemini
- real auth testing
- model listing
- effort controls

## Phase 4 — single model chat
- chat
- project context
- files
- streaming
- effort selector
- context indicator

## Phase 5 — context engine
- repository index
- Smart Context Cache
- compression
- deduplication
- Context Inspector
- log reduction
- handoff packet format

## Phase 6 — Combos
- Combo editor
- presets
- router
- usage strategy
- Usage Reserve
- effort escalation
- fallback

## Phase 7 — agent tools
- files
- terminal
- Git
- browser
- permissions
- Fast Stop
- Emergency Stop

## Phase 8 — Goal
- Goal Contract
- task graph
- roles
- verification
- Test Gate
- reassignment
- review

## Phase 9 — integrations
- MCP
- MCP Doctor
- Skills
- Plugins
- Caveman
- Environment Doctor
- tool discovery

## Phase 10 — GitHub / remote
- GitHub
- Conductor Host
- remote auth
- diff sync
- real-time state
- disconnect-safe goals

## Phase 11 — computer use / tunnels / previews
- computer control
- app scoping
- tunnel manager
- Preview Center
- screenshot testing

## Phase 12 — updates / releases
- signed updater
- rollback
- provider catalogs
- integration updates
- packaging
- installers
- portable mode

## Phase 13 — hardening
- performance
- security
- low-end testing
- crash recovery
- migration testing
- cross-platform validation
- docs
- release readiness

Do not fake later phases just to check boxes.

---

# 163. Acceptance criteria for v1

A serious v1 should demonstrate:

1. Install on at least Windows, macOS, and mainstream Linux.
2. Launch with a polished setup wizard and Conductor branding.
3. No mandatory Conductor account.
4. Connect at least the primary providers with supported auth methods.
5. OAuth paths that claim support are tested.
6. API-key paths are tested.
7. Open a repository.
8. Chat with one model.
9. Select a Combo.
10. Set effort cleanly.
11. Use context compression.
12. Reuse Smart Context Cache.
13. Show Context Budget.
14. Read/write files under permission policy.
15. Run terminal commands.
16. Run tests.
17. Use Git safely.
18. Use Goal mode.
19. Transfer a task between providers.
20. Continue after one provider is unavailable when policy allows.
21. Install/configure an MCP from inside Conductor.
22. MCP Doctor can diagnose a broken integration.
23. Install a skill.
24. Enable/disable Caveman.
25. Background Goal continues after UI closes.
26. Session resumes after reopening.
27. Full Access works without repetitive prompts.
28. Fast Stop works.
29. App remains low-resource when idle.
30. Remote host works without a Conductor account.
31. Real-time project changes sync remotely.
32. Updates are verified before activation.
33. No secrets are stored in plaintext.
34. No known critical security issue remains.
35. Documentation allows another developer to build from source.

---

# 164. UX success criteria

The app should feel understandable within a few minutes.

A new user should be able to:
- install
- connect one provider
- open a project
- choose a model
- type a request
- understand what is happening
- stop it if needed

without reading a manual.

Advanced users should be able to:
- build custom Combos
- tune routing
- configure MCP
- inspect context
- customize instructions
- use remote access
- install skills
- create themes

without the default UI becoming cluttered.

---

# 165. Final engineering priorities

When tradeoffs exist, prioritize in this order:

1. correctness
2. user control
3. security
4. context/token efficiency
5. reliability
6. speed
7. simplicity
8. cross-platform consistency
9. polish
10. extra features

Do not sacrifice correctness for a benchmark.

Do not sacrifice user control for automation.

Do not sacrifice security for convenience.

Do not sacrifice simplicity for showing off technical complexity.

---

# 166. Final product direction

Conductor is not “three chat windows in one app”.

Conductor is an AI development control plane.

The user says what they want.

Conductor:
- understands the goal
- asks only important questions
- picks efficient models
- uses the right effort
- routes tasks
- compresses context
- shares only relevant information
- preserves provider usage
- installs tools when needed
- coordinates agents
- verifies results
- falls back gracefully
- keeps working in the background
- stays fast
- stays local-first
- stays understandable

The complexity belongs underneath the product.

The surface should remain simple.

Build Conductor so someone can open it and think:

“This is obvious.”

Then let advanced users discover how powerful it is.

---

# 167. Instructions to the coding agent executing this prompt

Start by inspecting the repository.

If the repository is empty:
- initialize the project cleanly
- establish the Rust workspace
- create architecture docs
- choose and document the UI stack
- create the initial design system
- create the Conductor logo direction
- establish CI
- begin Phase 0 and Phase 1

If the repository already contains code:
- audit it first
- preserve working behavior
- identify gaps against this specification
- create a plan
- continue from the current state instead of restarting unnecessarily

Work autonomously where requirements are clear.

Ask the user only when a product decision is genuinely ambiguous and materially changes the result.

Do not ask for routine technical permission if the environment and user policy already authorize the work.

When multiple agents are working on the repository:
- coordinate scope
- use branches/worktrees as needed
- do not overwrite each other
- keep handoffs concise
- run tests
- preserve repository integrity

Do not claim completion until the relevant phase is actually implemented and tested.

The final product must be something another person can install and use.

That is the standard.

