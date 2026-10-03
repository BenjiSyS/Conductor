//! Security primitives shared by Conductor subsystems.
//!
//! * [`secrets`] — detect and redact credentials before context leaves the
//!   machine, and before anything is written to logs or history.
//! * [`paths`] — confine file access to a project root (path traversal and
//!   symlink-escape defence).
//! * [`integrity`] — SHA-256 checksums and Ed25519 signature verification for
//!   downloaded components, catalogs and updates.
//! * [`injection`] — mark retrieved content as untrusted data and flag
//!   instruction-like text so it can never silently override policy.
//! * [`shell`] — command-line validation used before spawning processes.

pub mod injection;
pub mod integrity;
pub mod paths;
pub mod secrets;
pub mod shell;
