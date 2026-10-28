//! Colophon's MCP server (docs/modules/09-automation.md, "MCP adapter"; D-066): typed tools for
//! agents over the Model Context Protocol, version 2025-11-25, on stdio.
//!
//! It is a client of the same HTTP API the Studio and the CLI use, authenticated by a project API
//! token (D-063), and never a privileged path: what the token may do is decided by the API on
//! every call. It offers reading content, writing drafts and asking people to review a publish
//! request; nothing approves or publishes (AU2: agents propose, the platform enforces).
//!
//! - [`jsonrpc`]: JSON-RPC 2.0 framing, one message per line, with a size limit.
//! - [`server`]: the session: the `initialize` handshake, `tools/list`, `tools/call`, cancellation.
//! - [`tools`]: each tool, its input schema, argument checks and error mapping.
//! - [`discovery`]: which tools to list, from what the API says the token may do.
//! - [`api`]: the HTTP client, with deadlines, size limits and safe retries.
//! - [`config`]: `COLOPHON_SERVER`, `COLOPHON_TOKEN` and `COLOPHON_PROJECT`.

pub mod api;
pub mod config;
pub mod discovery;
pub mod jsonrpc;
pub mod server;
pub mod tools;

pub use config::{Config, ConfigError};
pub use server::{PROTOCOL_VERSION, ServeError, serve};
