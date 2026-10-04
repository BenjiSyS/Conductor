# Contributing to Conductor

Read [architecture](docs/ARCHITECTURE.md) before proposing product changes. Preserve Conductor's name and calm, local-first experience.

Use a current stable Rust toolchain and Node.js 22+. Install Tauri's native prerequisites, then run `npm ci`.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run check
npm test
npm run build
npm run test:e2e
```

For backend development without desktop prerequisites, use `cargo test --workspace --exclude conductor-app`. This is narrower than the complete desktop gate; record the exact command used.

Keep changes scoped. Add behavioral tests for consequential logic, including failure and cancellation paths. Document external requirements and untested platform boundaries. Never claim OAuth, provider, remote, update, performance or cross-platform success from a mocked UI or compilation alone.

Do not include keys, user prompts, project files or sensitive logs in issues. Follow [SECURITY.md](SECURITY.md) for vulnerabilities. Follow [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) in project interactions.

If several people share a checkout, agree who owns which files and preserve unrelated changes. Do not reset or discard another person's work. Generated dependencies/build outputs are ignored; lockfiles are retained.
