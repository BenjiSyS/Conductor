//! Conductor's context engine.
//!
//! Goal: **use the minimum context necessary to preserve output quality.**
//!
//! Entry points:
//! * [`index::RepoIndex`] — incremental repository index backed by the
//!   persistent Smart Context Cache (content-hash keyed).
//! * [`pack::build`] — the multi-stage compression pipeline that turns a task,
//!   conversation and index into a [`pack::ContextPack`] with a full
//!   inspector report (what was included, omitted, summarised and why).
//! * [`logs::reduce`] — build/test log reduction.
//! * [`handoff::HandoffPacket`] — compact provider-independent task handoff.
//! * [`tokens::estimate`] — deterministic token estimation (always labelled
//!   as an estimate).
//!
//! Everything here is deterministic Rust; no model is required.

pub mod compact;
pub mod handoff;
pub mod index;
pub mod logs;
pub mod pack;
pub mod relevance;
pub mod symbols;
pub mod tokens;

pub use index::{IndexStats, RepoIndex};
pub use pack::{ContextPack, PackRequest};
