//! PostgreSQL implementation of the registration persistence port.
//!
//! The port's reads live in `registration_read`, its writes in
//! `registration_write` and the statements both build on in
//! `registration_statements`; this module owns the adapter and the error
//! and outcome translation every one of them shares.

use crate::{
    csource_registration::{registration_read, registration_write},
    error::violates_unique_constraint,
    store_fault::{StoreFault, persistence_failure},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_query::{csr::compiled::CompiledCsrQuery, page_window::PageWindow};
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use andromeda_registration::{
    error::RegistrationError,
    resolved_update::ResolvedCsrUpdate,
    store::{RegistrationStore, create_params::CreateCsrParams, forwarding_outcome::ForwardingOutcome, mutation::RegistrationMutation, page::RegistrationPage},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use sqlx::postgres::PgPool;
use std::sync::Arc;

/// The primary key migration 0012 declares on `context_source_registrations.id`,
/// the registration identifier itself.
const PK_CONTEXT_SOURCE_REGISTRATIONS: &str = "pk_context_source_registrations";

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
/// Classify an adapter failure at the port boundary: backpressure keeps the
/// port's retryable category and every fault becomes its persistence failure.
#[track_caller]
pub(super) fn persistence_error(error: impl StoreFault) -> RegistrationError {
    RegistrationError::from(persistence_failure(&error))
}

/// Preserve a uniqueness race on the identifier as the registration identifier
/// conflict.
///
/// Only the identifier's own constraint is the client's conflict. It is the
/// only unique constraint the registration relation declares today; matching
/// it by name keeps a constraint added later from being reported as a 409
/// naming an identifier nobody used.
pub(super) fn create_error(error: sqlx::Error, identifier: &Identifier) -> RegistrationError {
    if violates_unique_constraint(&error, PK_CONTEXT_SOURCE_REGISTRATIONS) {
        RegistrationError::already_exists(identifier)
    } else {
        persistence_error(error)
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
    u64::try_from(total).map_err(|error| persistence_error(error))
}

#[async_trait]
impl RegistrationStore for PostgresRegistrationStore {
    async fn delete_expired(&self, cutoff: DateTime<Utc>) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        registration_write::delete_expired(self, cutoff).await
    }

    async fn find_expired(&self, cutoff: DateTime<Utc>) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        registration_read::find_expired(self, cutoff).await
    }

    async fn exists(&self, id: &Identifier) -> Result<bool, RegistrationError> {
        registration_read::exists(self, id).await
    }

    async fn create(&self, id: &Identifier, params: &CreateCsrParams) -> Result<(), RegistrationError> {
        registration_write::create(self, id, params).await
    }

    async fn get(&self, id: &Identifier) -> Result<Option<ContextSourceRegistration>, RegistrationError> {
        registration_read::get(self, id).await
    }

    async fn query_compiled(&self, compiled: &CompiledCsrQuery, window: &PageWindow) -> Result<RegistrationPage, RegistrationError> {
        registration_read::query_compiled(self, compiled, window).await
    }

    async fn update(&self, id: &Identifier, values: &ResolvedCsrUpdate) -> Result<RegistrationMutation, RegistrationError> {
        registration_write::update(self, id, values).await
    }

    async fn find_matching(&self, entity_type: Option<&IriBuf>, entity_id: Option<&Identifier>) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        registration_read::find_matching(self, entity_type, entity_id).await
    }

    async fn find_overlapping(&self, entity_id: Option<&Identifier>, attributes: &[IriBuf]) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        registration_read::find_overlapping(self, entity_id, attributes).await
    }

    async fn find_all_active(&self, after: Option<&Identifier>, limit: u32) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        registration_read::find_all_active(self, after, limit).await
    }

    async fn record_forwarding_outcome(&self, id: &Identifier, outcome: ForwardingOutcome, at: DateTime<Utc>) -> Result<(), RegistrationError> {
        registration_write::record_forwarding_outcome(self, id, outcome, at).await
    }

    async fn delete(&self, id: &Identifier) -> Result<RegistrationMutation, RegistrationError> {
        registration_write::delete(self, id).await
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        csource_registration::registration_store::{PK_CONTEXT_SOURCE_REGISTRATIONS, create_error, mutation_outcome},
        migration_set::tenant::tenant_migrator,
    };
    use andromeda_registration::{error::RegistrationError, store::mutation::RegistrationMutation};
    use pretty_assertions::assert_eq;
    use std::{
        fmt::Debug,
        sync::{Arc, Mutex},
    };
    use tracing::{
        Event,
        Metadata,
        Subscriber,
        callsite::rebuild_interest_cache,
        field::{Field, Visit},
        span::{Attributes, Id, Record},
        subscriber::with_default,
    };

    /// Migration 0012 names every constraint, so the duplicate-identifier
    /// refusal matches the declared name rather than one PostgreSQL generated.
    #[test]
    fn the_duplicate_id_refusal_names_the_constraint_migration_0012_declares() {
        let declaration = format!("CONSTRAINT {PK_CONTEXT_SOURCE_REGISTRATIONS} PRIMARY KEY (id)").to_ascii_uppercase();
        assert!(
            tenant_migrator()
                .iter()
                .any(|migration| migration.sql.as_str().to_ascii_uppercase().contains(&declaration)),
            "{declaration}"
        );
    }

    #[test]
    fn a_create_failure_that_is_no_unique_violation_stays_a_persistence_fault() {
        let identifier = "urn:ngsi-ld:ContextSourceRegistration:1".parse().expect("a valid identifier");
        assert!(matches!(create_error(sqlx::Error::RowNotFound, &identifier), RegistrationError::Persistence(_)));
        assert!(matches!(
            create_error(sqlx::Error::PoolTimedOut, &identifier),
            RegistrationError::StoreSaturated
        ));
    }

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
        assert!(matches!(persistence_error(sqlx::Error::PoolTimedOut), RegistrationError::StoreSaturated));
        assert!(matches!(persistence_error(sqlx::Error::RowNotFound), RegistrationError::Persistence(_)));
    }

    #[test]
    fn a_lost_connection_becomes_store_unavailable() {
        use crate::csource_registration::registration_store::persistence_error;
        use std::io::{Error as IoError, ErrorKind};
        assert!(matches!(
            persistence_error(sqlx::Error::Io(IoError::from(ErrorKind::ConnectionReset))),
            RegistrationError::StoreUnavailable
        ));
        assert!(matches!(persistence_error(sqlx::Error::RowNotFound), RegistrationError::Persistence(_)));
    }

    /// Every location the classification is recorded with.
    struct RecordedLocations {
        locations: Arc<Mutex<Vec<String>>>,
    }

    /// Collects the `location` field of every event.
    struct LocationVisitor<'a>(&'a mut Vec<String>);

    impl Visit for LocationVisitor<'_> {
        fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
            if field.name() == "location" {
                self.0.push(format!("{value:?}"));
            }
        }
    }

    impl Subscriber for RecordedLocations {
        fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _span: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }

        fn record(&self, _span: &Id, _values: &Record<'_>) {}

        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

        fn event(&self, event: &Event<'_>) {
            if let Ok(mut locations) = self.locations.lock() {
                event.record(&mut LocationVisitor(&mut locations));
            }
        }

        fn enter(&self, _span: &Id) {}

        fn exit(&self, _span: &Id) {}
    }

    /// The classification is recorded at the statement that failed, which is
    /// the operator's only pointer to it during an outage: a store call site
    /// reaches the helper through a closure, never by passing the helper to
    /// `map_err` by name, which would record a location inside `core`.
    #[test]
    fn the_failure_is_recorded_at_the_store_call_site() {
        use crate::csource_registration::registration_store::persistence_error;
        let locations = Arc::new(Mutex::new(Vec::new()));
        let subscriber = RecordedLocations {
            locations: Arc::clone(&locations),
        };
        with_default(subscriber, || {
            rebuild_interest_cache();
            let failed: Result<(), sqlx::Error> = Err(sqlx::Error::RowNotFound);
            failed.map_err(|error| persistence_error(error))
        })
        .expect_err("the statement failed");
        let locations = locations.lock().expect("the test lock is not poisoned");
        assert!(!locations.is_empty(), "the failure was not recorded");
        assert!(locations.iter().all(|location| location.contains("registration_store.rs")), "{locations:?}");
    }
}
