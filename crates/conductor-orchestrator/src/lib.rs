//! Orchestration logic for Conductor.
//!
//! This crate is provider-neutral and I/O-free except through the traits in
//! [`runner`]: the integrator supplies a [`runner::ModelClient`] (talks to
//! real providers) and a [`runner::Verifier`] (runs Test Gate commands), and
//! this crate decides *who* does *what* at *which effort*, keeps the Goal
//! Contract authoritative, and refuses to mark a Goal complete without
//! verification evidence.
//!
//! Modules:
//! * [`model`] — model profiles, capabilities, tiers, effort levels.
//! * [`combo`] — Combos and adaptive presets.
//! * [`router`] — strategy-, health-, usage- and reserve-aware routing.
//! * [`usage`] — per-provider usage tracking (estimates labelled as such).
//! * [`effort`] — automatic effort with conservative escalation and max-effort gating.
//! * [`clarify`] — the clarifying-question system (≤ 7 per round).
//! * [`goal`] — Goal Contract, task graph, Definition of Done.
//! * [`ledger`] — duplicate-work prevention and disagreement summaries.
//! * [`runner`] — Goal execution loop with fallback and reassignment.

pub mod clarify;
pub mod combo;
pub mod effort;
pub mod goal;
pub mod ledger;
pub mod model;
pub mod roles;
pub mod router;
pub mod runner;
pub mod usage;

pub use combo::{Combo, ComboMember};
pub use model::{EffortLevel, ModelProfile, Tier};
pub use roles::Role;
