<p align="center"><img src="apps/desktop/ui/public/logo.svg" width="80" alt="Conductor logo"></p>

# Conductor

An open-source, local-first workspace for AI-assisted development. Choose a model or reusable Combo, work with a project, and keep context, permissions and verification visible. No Conductor account.

**Development status:** early releases. Builds are not code-signed yet, and some integrations are verified only on Windows so far — see [releases](docs/RELEASES.md).

## Download

Get the installer for your computer from the [latest release](https://github.com/BenjiSyS/Conductor/releases): `.exe`/`.msi` for Windows 10 and 11, `.dmg` for macOS (Apple Silicon or Intel), `.deb` for Debian, Ubuntu and Kali, `.rpm` for Fedora and openSUSE, and `.AppImage` for any other Linux. Builds are not code-signed yet, so the first launch shows a system warning.

## Build from source

You need Git, Node.js 22 or newer, Rust 1.85 or newer, and your system's desktop build tools. Install them once with the commands for your system, then build.

### 1. Install the tools

**Windows 10 / 11** (PowerShell):

```powershell
winget install Git.Git OpenJS.NodeJS.LTS Rustlang.Rustup
winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

WebView2 is already part of Windows 10 and 11. If it's missing: `winget install Microsoft.EdgeWebView2Runtime`. Close and reopen the terminal afterwards so the new tools are on your PATH.

**macOS**:

```sh
xcode-select --install
brew install node
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**Ubuntu, Debian, Kali, Mint, Pop!_OS**:

```sh
sudo apt update && sudo apt install -y git build-essential curl wget file pkg-config libwebkit2gtk-4.1-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Then install Node.js 22 or newer from [nodejs.org](https://nodejs.org) (the version in many distributions is too old).

**Fedora**:

```sh
sudo dnf install -y git webkit2gtk4.1-devel openssl-devel curl wget file libappindicator-gtk3-devel librsvg2-devel libxdo-devel nodejs
sudo dnf group install -y c-development
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**Arch Linux**:

```sh
sudo pacman -S --needed git nodejs npm webkit2gtk-4.1 base-devel curl wget file openssl appmenu-gtk-module libappindicator-gtk3 librsvg xdotool rustup
rustup default stable
```

### 2. Get the code and check your setup

```sh
git clone https://github.com/BenjiSyS/Conductor.git
cd Conductor
npm ci
npm run doctor
```

`npm run doctor` lists anything that's still missing and the exact command to install it.

### 3. Run or build

```sh
npm run desktop          # start the app in development mode
npm run desktop:build    # build installers for this computer
```

The first build takes 5–15 minutes. Installers are written to `target/release/bundle/` (`.exe`/`.msi` on Windows, `.dmg` on macOS, `.deb`/`.rpm`/`.AppImage` on Linux). These local builds aren't code-signed. `npm run desktop:release` makes the signed release build and needs the maintainers' updater signing key.

To check that a build or an installed copy really starts, run `bash scripts/release-smoke.sh <path-to-conductor-app>` (on Linux under `xvfb-run` if there's no display).

Run the headless CLI:

```sh
cargo run -p conductor-cli -- --help
cargo run -p conductor-cli -- doctor
cargo run -p conductor-cli -- context . "explain provider streaming" --json
```

The browser development server (`npm run dev`, http://127.0.0.1:1420) is for frontend work only. It shows sample data; projects, credentials and models need the desktop app.

## First use

1. Choose a performance profile in setup, or configure it later.
2. Connect a provider with an API key through Settings. Keys use OS credential storage, with no plaintext fallback.
3. Open a project folder.
4. Choose an available model or Combo and an effort supported by that model.
5. Send a request. Use Stop to cancel; the emergency shortcut is configurable in the desktop application.

Provider requests send the selected prompt and context to that provider. Review the Context Inspector and [privacy documentation](docs/PRIVACY.md). Secret detection reduces exposure but cannot identify every possible secret.

Ways to connect a model:

- **API key:** OpenAI, Anthropic, Google Gemini, xAI (Grok) and any OpenAI-compatible endpoint.
- **An AI app you're already signed in to (no key):** Antigravity CLI (Gemini), Codex CLI (ChatGPT) or Claude Code. Conductor runs the app the way you would in a terminal and never reads its credentials. Your plan is preferred by routing because it's already paid for.
- **Local models, fully offline:** Ollama or LM Studio are found automatically during setup. With a local model, Conductor needs no internet at all.

**Usage** (Settings › Usage, or the profile menu) shows provider-reported limits per provider in that provider's own theme. An opt-in, unsupported sign-in for Claude, ChatGPT and Gemini plans adds 5-hour and weekly meters, with a desktop notification below 10% left. Remote **MCP connectors** connect by URL; when a connector needs an account its sign-in page opens (MCP OAuth with automatic client registration). See [provider adapters and authentication](docs/PROVIDERS.md).

## Working concepts

- **Chat / Plan / Agent / Goal:** questions, planning, permitted tools and outcome-oriented orchestration. A Goal completes only after its configured acceptance checks.
- **Combos:** reusable model roles, routing, reserves and fallback policy; providers are not permanently tied to a role.
- **Context:** selected repository data, hash-keyed cache, compression, estimated token budget and inspectable handoff packets.
- **Permissions:** Ask, Auto Approve and Full Access. Full Access does not bypass path or download integrity checks.
- **History:** meaningful decisions, handoffs and verification. Checkpoints preserve project work before risky changes.
- **Background / resume:** the desktop lifecycle and durable Goal state support continuation; native lifecycle verification remains a release gate.

These are product concepts, not a blanket claim that every integration has passed release validation. Current evidence is recorded per requirement.

## Extend and contribute

[MCP](docs/MCP.md) · [Skills and plugins](docs/EXTENSIONS.md) · [Create a theme](docs/THEMES.md) · [Architecture](docs/ARCHITECTURE.md) · [Security](SECURITY.md) · [Contributing](CONTRIBUTING.md) · [Releases](docs/RELEASES.md)

Community themes use a validated `theme.json` data format. Start with [the theme template](examples/theme/theme.json), install the local folder in Appearance settings, and preview both light and dark variants. See the dedicated theme guide for packaging, versioning and trust.

No mandatory cloud registration, forced telemetry, Quiet Mode, community template marketplace or provider lock-in.

MIT license. Third-party components retain their own licenses. Redistribution and signing checks remain part of release readiness.
