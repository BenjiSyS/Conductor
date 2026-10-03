//! Conductor engine: wires the provider adapters in `conductor-core` to the
//! orchestration, context and tool crates.
//!
//! * [`profiles`] — map configured provider models to orchestration profiles.
//! * [`toolbox`] — the provider-neutral agent tool protocol and executor
//!   (works with every provider because it is plain text, not a vendor
//!   function-calling schema).
//! * [`agent`] — the agent loop and the [`agent::CoreModelClient`] used by
//!   Goal execution.
//! * [`approvals`] — pending permission requests the UI answers.
//! * [`combos`] — Combo storage with adaptive presets.
//! * [`goals`] — the Goal service: persistent, resumable, stoppable Goals.
//! * [`verifier`] — Test Gate verifier.

pub mod agent;
pub mod approvals;
pub mod catalog;
pub mod combos;
pub mod events;
pub mod goals;
pub mod memory;
pub mod paths;
pub mod profiles;
pub mod subscriptions;
pub mod toolbox;
pub mod verifier;

pub use events::EngineEvent;
