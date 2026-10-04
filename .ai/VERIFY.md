# Verification

## Install
`npm ci`

## Build
`npm run build`
`cargo build --workspace`
`npm run desktop:build`
Unsigned development package: `npx tauri build --config .github/unsigned-tauri.json -- --locked`

## Unit tests
`cargo test --workspace`
`npm test`
`node --test scripts/release-evidence.test.mjs`

## Typecheck
`npm run check`

## Lint
`cargo clippy --workspace --all-targets -- -D warnings`

## Format/check
`cargo fmt --all -- --check`
`npm run format:check`

## Dev server
`npm run dev` (http://127.0.0.1:1420)
`npm run desktop`

## Browser/e2e
`npm run test:e2e`

## Dependency audit
`npm audit --audit-level=moderate`
`cargo audit --deny unsound`

Commands are configured. Passing evidence belongs in docs/VERIFICATION.md; configuration alone is not verification.
