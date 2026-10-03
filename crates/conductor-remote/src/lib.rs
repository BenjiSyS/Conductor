//! Conductor Host — remote access without any central account.
//!
//! The host runs on the machine that has the project. Remote clients (a
//! laptop, a phone browser, another Conductor) connect directly:
//!
//! * **Encryption**: TLS with a host-generated self-signed certificate. The
//!   client pins the certificate's SHA-256 fingerprint, which it learns
//!   out-of-band during pairing (shown on the host screen). No CA, no cloud.
//! * **Trust**: short-lived single-use pairing codes create a device with a
//!   random bearer token (stored only as a hash). Devices are listed,
//!   scoped to specific projects, and revocable instantly — revocation also
//!   closes their live connections.
//! * **State channel**: one WebSocket per client carries chat, task status,
//!   permission prompts, terminal output, file changes and provider state.
//! * **File sync**: files are versioned by content hash; writes must name the
//!   base hash they edit, so concurrent edits are detected (409) instead of
//!   silently overwritten. Changes are pushed as diffs when practical.
//! * **Audit**: pairing, connections, revocations and writes are logged.
//!
//! The host keeps working (and approved Goals keep running) when clients
//! disconnect; it only needs to be on and reachable.

pub mod audit;
pub mod client;
pub mod devices;
pub mod host;
pub mod sync;
pub mod tls;

pub use host::{Host, HostConfig, HostHandle, Inbound};

pub(crate) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
