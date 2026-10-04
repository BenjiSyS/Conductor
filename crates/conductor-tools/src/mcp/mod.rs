//! Model Context Protocol support.
//!
//! * [`config`] — one shared MCP configuration, exported to each provider's
//!   native format (Claude, Codex, Gemini) so a server is configured once.
//! * [`client`] — JSON-RPC 2.0 stdio client (initialize, tools/list,
//!   tools/call).
//! * [`doctor`] — MCP Doctor: diagnose and suggest fixes.
//! * [`catalog`] — known servers with pinned, verifiable sources for
//!   "Install this MCP" requests.

pub mod catalog;
pub mod client;
pub mod config;
pub mod doctor;
pub mod session;

pub use config::{McpConfig, McpServer, Transport};
