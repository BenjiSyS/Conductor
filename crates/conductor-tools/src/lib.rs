//! Tools that Conductor agents use, behind one consistent shape: every tool
//! call has an identity, input, declared capabilities, result, logs,
//! cancellation state and provenance ([`call::ToolCall`]).
//!
//! * [`exec`] — spawn processes with argv (no shell strings), timeouts,
//!   cancellation (whole process tree) and bounded output.
//! * [`git`] — safe Git operations; destructive commands are not exposed.
//! * [`checkpoint`] — lightweight restorable snapshots via Git objects that
//!   never touch the user's index, branches or stash.
//! * [`mcp`] — MCP stdio client, shared config, installer and MCP Doctor.
//! * [`skills`], [`plugins`], [`themes`] — manifest-based extensions with
//!   explicit permissions, verified installs, updates and rollback.
//! * [`receipts`] — installation receipts and clean removal.
//! * [`envdoctor`] — detect toolchains and diagnose problems.
//! * [`caveman`] — the Caveman optimisation component manager.
//! * [`tunnels`] — local port detection and tunnel lifecycle.
//! * [`project`] — project detection, `conductor.toml`, setup recipes.
//! * [`testgate`] — run Definition-of-Done commands with reduced evidence.

pub mod call;
pub mod caveman;
pub mod checkpoint;
pub mod envdoctor;
pub mod exec;
pub mod git;
pub mod mcp;
pub mod package;
pub mod plugins;
mod process_lifetime;
pub mod project;
pub mod receipts;
pub mod skills;
pub mod testgate;
pub mod themes;
pub mod tunnels;

/// Current unix time in seconds.
pub(crate) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
