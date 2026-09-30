//! The schema contract one tenant database carries, and what it is checked against.
//!
//! Database-per-tenant means the tenant schema exists in as many copies as
//! there are tenants, and every copy can drift: a migration that fails
//! mid-fleet, a tenant restored from an older backup, manual surgery. The
//! `schema_metadata` singleton is the stamp the broker writes once the embedded
//! migrator has finished and re-reads before the tenant's pool is published, so
//! drift is refused there rather than discovered by the first failing query.
//!
//! The stamp is deliberately not written by a migration: a migration cannot
//! compute the digest of the migration set it belongs to, so the value is
//! derived in Rust from the embedded migrator and never from the filesystem.

/// The pair a tenant database is stamped with.
pub mod contract;
/// What a schema-contract check refuses a tenant for.
pub mod error;
/// The digest of the migration set a broker binary embeds.
pub mod migrations_digest;
/// The `schema_metadata` singleton: stamping it, reading it, and verifying it.
pub mod schema_metadata;
/// The stamped schema version.
pub mod schema_version;
/// The server-side character sets a PostgreSQL database can be created with.
pub mod server_encoding;
/// The server and extension versions the tenant schema depends on.
pub mod server_requirements;
