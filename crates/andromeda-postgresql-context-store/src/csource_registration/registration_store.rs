//! PostgreSQL implementation of the registration persistence port.
//!
//! The port's reads live in `registration_read`, its writes in
//! `registration_write` and the statements both build on in
//! `registration_statements`; this module owns the adapter and the error
//! and outcome translation every one of them shares.

use crate::csource_registration::{registration_read, registration_write};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_query::{csr::compiled::CompiledCsrQuery, pagination::Pagination};
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use andromeda_postgresql_pool::is_pool_saturated;
use andromeda_registration::{
    error::RegistrationError,
    resolved_update::ResolvedCsrUpdate,
    store::{RegistrationStore, create_params::CreateCsrParams, forwarding_outcome::ForwardingOutcome, mutation::RegistrationMutation, page::RegistrationPage},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use sqlx::postgres::PgPool;
use std::{any::Any, fmt::Display, sync::Arc};

/// PostgreSQL-backed registration persistence adapter.
///
/// Holds two representations of the same facts: the registration payload, which
/// every response body is rendered from, and the `registration_information`
/// routing projection derived from its `information`, which discovery
/// preselects against. Every mutation below rewrites the projection in the same
/// transaction as the payload, so no reader observes one without the other.
#[derive(Debug)]
pub struct PostgresRegistrationStore {
    /// Tenant-scoped PostgreSQL connection pool.
    pub(super) pool: PgPool,
    /// The tenant's attribute-name dictionary, which the routing projection
    /// interns registered property and relationship names through.
    pub(super) attribute_names: Arc<dyn NameLookup<AttributeName>>,
}

impl PostgresRegistrationStore {
    /// Create a registration store backed by the supplied tenant pool and the
    /// tenant's shared attribute-name dictionary.
    pub fn new(pool: &PgPool, attribute_names: Arc<dyn NameLookup<AttributeName>>) -> PostgresRegistrationStore {
        PostgresRegistrationStore {
            pool: pool.clone(), // clone: `PgPool` is an atomically reference-counted pool handle.
            attribute_names,
        }
    }
}

/// Erase an adapter-specific diagnostic at the policy error boundary.
///
/// A `sqlx` pool-acquire timeout is backpressure, not a fault, so it becomes
/// [`RegistrationError::StoreSaturated`] (a 503); every other failure stays a
/// persistence fault. The concrete [`sqlx::Error`] is recovered by downcast
/// because callers hand it in already erased behind `impl Display`.
pub(super) fn persistence_error(error: impl Display + Any) -> RegistrationError {
    match (&error as &dyn Any).downcast_ref::<sqlx::Error>() {
        Some(sqlx_error) if is_pool_saturated(sqlx_error) => RegistrationError::StoreSaturated,
        Some(_) | None => RegistrationError::persistence(error),
    }
}

/// Preserve PostgreSQL unique violations as registration identifier conflicts.
pub(super) fn create_error(error: sqlx::Error, identifier: &Uri) -> RegistrationError {
    match &error {
        sqlx::Error::Database(database_error) if database_error.code().as_deref() == Some("23505") => RegistrationError::already_exists(identifier),
        sqlx::Error::Configuration(_)
        | sqlx::Error::Database(_)
        | sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::Protocol(_)
        | sqlx::Error::RowNotFound
        | sqlx::Error::TypeNotFound { .. }
        | sqlx::Error::ColumnIndexOutOfBounds { .. }
        | sqlx::Error::ColumnNotFound(_)
        | sqlx::Error::ColumnDecode { .. }
        | sqlx::Error::Encode(_)
        | sqlx::Error::Decode(_)
        | sqlx::Error::AnyDriverError(_)
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed
        | sqlx::Error::Migrate(_)
        | sqlx::Error::InvalidSavePointStatement
        | sqlx::Error::BeginFailed
        | sqlx::Error::InvalidArgument(_)
        | _ => persistence_error(error),
    }
}

/// Map PostgreSQL affected-row counts to a backend-neutral mutation outcome.
pub(super) const fn mutation_outcome(rows_affected: u64) -> RegistrationMutation {
    match rows_affected {
        0 => RegistrationMutation::Missing,
        1.. => RegistrationMutation::Applied,
    }
}

/// Convert PostgreSQL's signed count representation to the policy-owned count.
pub(super) fn registration_count(total: i64) -> Result<u64, RegistrationError> {
    u64::try_from(total).map_err(persistence_error)
}

/// Add the full registration row projection to `select`, decoding each stored
/// geometry back to `GeoJSON` and each enum to text under the column name the
/// mapper reads it by.

#[async_trait]
impl RegistrationStore for PostgresRegistrationStore {
    async fn delete_expired(&self, cutoff: DateTime<Utc>) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        registration_write::delete_expired(self, cutoff).await
    }

    async fn find_expired(&self, cutoff: DateTime<Utc>) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        registration_read::find_expired(self, cutoff).await
    }

    async fn exists(&self, id: &Uri) -> Result<bool, RegistrationError> {
        registration_read::exists(self, id).await
    }

    async fn create(&self, id: &Uri, params: &CreateCsrParams) -> Result<(), RegistrationError> {
        registration_write::create(self, id, params).await
    }

    async fn get(&self, id: &Uri) -> Result<Option<ContextSourceRegistration>, RegistrationError> {
        registration_read::get(self, id).await
    }

    async fn list(&self, pagination: Pagination) -> Result<RegistrationPage, RegistrationError> {
        registration_read::list(self, pagination).await
    }

    async fn query_compiled(&self, compiled: &CompiledCsrQuery) -> Result<RegistrationPage, RegistrationError> {
        registration_read::query_compiled(self, compiled).await
    }

    async fn update(&self, id: &Uri, values: &ResolvedCsrUpdate) -> Result<RegistrationMutation, RegistrationError> {
        registration_write::update(self, id, values).await
    }

    async fn find_matching(&self, entity_type: Option<&IriBuf>, entity_id: Option<&Uri>) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        registration_read::find_matching(self, entity_type, entity_id).await
    }

    async fn find_overlapping(&self, entity_id: Option<&Uri>, attributes: &[IriBuf]) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        registration_read::find_overlapping(self, entity_id, attributes).await
    }

    async fn find_all_active(&self, after: Option<&Uri>, limit: u32) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        registration_read::find_all_active(self, after, limit).await
    }

    async fn record_forwarding_outcome(&self, id: &Uri, outcome: ForwardingOutcome, at: DateTime<Utc>) -> Result<(), RegistrationError> {
        registration_write::record_forwarding_outcome(self, id, outcome, at).await
    }

    async fn delete(&self, id: &Uri) -> Result<RegistrationMutation, RegistrationError> {
        registration_write::delete(self, id).await
    }
}

#[cfg(test)]
mod tests {
    use crate::csource_registration::registration_store::mutation_outcome;
    use andromeda_registration::store::mutation::RegistrationMutation;
    use pretty_assertions::assert_eq;

    /// Zero affected rows preserve the policy-level missing outcome.
    #[test]
    fn zero_affected_rows_map_to_missing() {
        assert_eq!(mutation_outcome(0), RegistrationMutation::Missing);
    }

    /// Any positive affected-row count preserves the policy-level applied outcome.
    #[test]
    fn positive_affected_rows_map_to_applied() {
        assert_eq!(mutation_outcome(1), RegistrationMutation::Applied);
        assert_eq!(mutation_outcome(2), RegistrationMutation::Applied);
    }

    #[test]
    fn a_pool_timeout_becomes_store_saturated() {
        use crate::csource_registration::registration_store::persistence_error;
        use andromeda_registration::error::RegistrationError;
        assert!(matches!(persistence_error(sqlx::Error::PoolTimedOut), RegistrationError::StoreSaturated));
        assert!(matches!(persistence_error(sqlx::Error::RowNotFound), RegistrationError::Persistence(_)));
    }
}
