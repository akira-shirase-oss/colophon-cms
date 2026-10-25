//! Colophon's management API, `/api/v1`: one surface with one error format,
//! preconditions and idempotency, shared by the Studio, the CLI, the SDK and
//! agents, so no interface can bypass the rules.
//!
//! Design: `docs/modules/04-api.md`. Nothing is implemented yet; the error
//! envelope and health checks arrive in milestone P0, step 0.3.
