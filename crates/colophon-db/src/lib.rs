//! Colophon's PostgreSQL adapter: constraints and transactions that make
//! invalid states impossible to store, the transactional outbox, idempotency
//! records and row-level security context.
//!
//! Design: `docs/modules/02-db.md`. Nothing is implemented yet; migration
//! tooling and database roles arrive in milestone P0.
