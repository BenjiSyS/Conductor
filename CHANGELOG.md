# Changelog

## 0.3.0

- Installers for every system, each one installed and started by CI before it counts: Windows (x64 and ARM64) .exe/.msi, macOS (Apple Silicon and Intel) .dmg, Linux (x64 and ARM64) .deb, .rpm and .AppImage, tested on Ubuntu, Debian, Kali, Fedora and Arch.
- Building from source works on a new computer: exact install commands per OS in the README and `npm run doctor`; `npm run desktop:build` needs no signing key.
- Grok Build joins Codex CLI, Claude Code and Antigravity as a signed-in app (no API key), with Sign in buttons that open each provider's own sign-in page; Grok in Combos with effort levels; MCP export for Grok Build.
- Newest models always: connected apps update themselves daily and model lists refresh automatically (e.g. GPT-6.1 Sol); effort levels for GPT-6, Grok and Claude models.
- Fixes: Codex requests with an effort level on Windows, large pastes in /goal, Claude replies from the bridge.

## 0.2.0

- Only Chat and Agent are modes; type `/goal` or `/plan` (or pick them from the `/` menu) to start a Goal or a Plan.
- Connect AI apps you're already signed in to (Antigravity CLI, Codex CLI, Claude Code) without an API key; xAI (Grok) provider.
- Model lists refresh automatically (start-up and every 6 hours) and from Settings › Providers.
- Usage page per provider in its own theme, opt-in plan sign-in with 5-hour/weekly meters, low-usage and work-finished notifications with an optional ding.
- Profile menu with usage at a glance and a System / Light / Dark switch.
- Effort sliders, subscription-first routing, parallel agents that never edit the same file at once.
- Setup wizard checks the computer in the background and finds local (offline) models.
- Very large pastes, remote MCP connectors with one-click sign-in, installers for Windows, macOS and Linux.

## 0.1.0

- First build: Rust workspace, provider adapters, context, orchestration, tools, remote access, CLI and the desktop app.

These builds are not code-signed yet, and no release has passed every acceptance gate.
