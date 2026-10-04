# Skills and plugins

Skills and plugins are local, versioned packages with explicit manifests, source information, permissions and update policy. A skill can provide instructions and optional scripts, schemas, tools and MCP requirements. A plugin is a broader extension. Installing a package must not grant every capability implicitly.

Core package management is in `crates/conductor-tools/src/package.rs`, with separate skill and plugin modules. Installation receipts record package identity, source, version, location, integrity and removal. Removal may delete only files owned by that installation.

Use verified sources and integrity checks. Keep a known-good version for rollback. Activate updates at a safe task boundary. Compatibility failure should disable the affected adapter, not the entire application.

Caveman is a separate upstream optimization integration. Before redistribution, verify upstream identity, license, packaging and update source. Do not claim a token-reduction percentage or upstream update success from a local instruction string. Conductor's own context system must remain independent.

Required tests include malformed manifests, unauthorized permissions, tampered packages, interrupted installation, update rollback and clean removal. Native UI installation and real MCP dependencies require their own evidence.
