//! Placeholder for persistence-shaped snapshot row structs.
//!
//! The snapshot reader currently uses `sqlx::postgres::PgRow` directly (see
//! [`crate::repository::snapshot::mapper::row_to_snapshot`]) because several
//! columns are JSON blobs decoded on the fly.
