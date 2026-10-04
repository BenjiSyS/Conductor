# Releases

There is no validated production release yet. Source builds and locally produced packages are development artifacts until all applicable requirements pass.

## Build and package

On each supported operating system, install Tauri prerequisites and run:

```sh
npm ci
npm run build
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
npm run desktop:build
```

Artifacts are produced under the Cargo target release bundle directories. Test clean installation, launching, onboarding, credential storage, project open, update registration, uninstall and data preservation on the target OS. Compilation on Windows does not establish macOS/Linux installation.

## Development package automation

`.github/workflows/packages.yml` is a manual workflow for Windows x64, Linux x64 and macOS ARM64/x64 development packages. It runs source checks, builds with updater artifacts disabled, records SHA-256 hashes and build provenance, and uploads workflow artifacts. It does not publish a GitHub release. This workflow is configured, not remotely verified.

Its `.github/unsigned-tauri.json` override changes only updater artifact creation; production updater verification remains configured in the normal desktop configuration. Development packages are unsigned on Windows/Linux; macOS uses ad-hoc signing, without notarization or a trusted distribution certificate.

To reproduce locally:

```sh
npx tauri build --config .github/unsigned-tauri.json -- --locked
node scripts/release-evidence.mjs target/release/bundle
```

The evidence script streams package bytes through SHA-256 and refuses empty output. Its manifest explicitly leaves installation and signature verification false. Checksums are integrity metadata, not proof of trusted origin. SBOM, attestation, clean installation and signed publication remain release work.

## Release gates

- Close the 35 v1 acceptance gates and the full implementation audit with direct evidence.
- Run format, lint, tests, dependency audit, frontend checks, packaging and native smoke tests.
- Validate all dependency licenses and retain required third-party notices.
- Measure startup, idle CPU/RAM and low-resource behavior; never guess performance.
- Verify signing, update signature/checksum, interrupted download, failed activation, rollback and downgrade policy.
- Publish platform packages, checksums, changelog, SBOM and provenance where available.

Windows code signing, macOS signing/notarization, updater keys and release infrastructure are external requirements. Signing credentials must be CI secrets or secure local vault entries, never project files. Unsigned local packages must be labelled unsigned.

Application updates, model catalogs, themes, plugins, skills and Caveman have separate lifecycle and trust checks. Do not hot-swap an integration during an active Goal.

Portable installations must keep their configuration separate and expose the data location. Test the `portable.flag` behavior against the native desktop build before advertising it as verified.
