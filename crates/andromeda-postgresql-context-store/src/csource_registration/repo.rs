//! SQL-only repository for context source registrations.
//!
//! The `RegistrationStore` trait impl lives in the binary crate, because the
//! trait and its associated types are defined in the binary's service layer.
//! This module provides the struct, constructor, and the sea-query helpers
//! that compose the column projection.

use sea_query::{Alias, Expr as SeaExpr, Query, SelectStatement};
use sqlx::postgres::PgPool;

/// Projection for a full CSR row. Static, identifier-only expression -- no user
/// values -- applied to every read path so `ST_AsGeoJSON` geometry casts stay
/// in one place.
pub fn csr_column_projection() -> sea_query::SimpleExpr {
    SeaExpr::cust(
        "id, type, registration_name, description, information, \
         tenant, observation_interval, management_interval, \
         ST_AsGeoJSON(location)::JSONB AS location, \
         ST_AsGeoJSON(observation_space)::JSONB AS observation_space, \
         ST_AsGeoJSON(operation_space)::JSONB AS operation_space, \
         expires_at, endpoint, accept, mode::text AS mode, \
         operations, scope, refresh_rate, \
         management, context_source_info, jsonld_context, context_source_alias, \
         status::text AS status, times_sent, times_failed, \
         last_success, last_failure, created_at, modified_at, \
         additional_properties",
    )
}

/// Build a base `SELECT` over the `context_source_registrations` table with
/// the full column projection.
pub fn csr_base_select() -> SelectStatement {
    let mut select = Query::select();
    select.expr(csr_column_projection()).from(Alias::new("context_source_registrations"));
    select
}

/// PostgreSQL-backed repository for NGSI-LD context source registrations.
#[derive(Debug)]
pub struct CsourceRegistrationRepository {
    /// The tenant connection pool.
    pool: PgPool,
}

impl CsourceRegistrationRepository {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: &PgPool) -> CsourceRegistrationRepository {
        CsourceRegistrationRepository { pool: pool.clone() } // clone: Arc ref-count bump
    }

    /// Borrow the pool for trait impls defined outside this crate.
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }
}
