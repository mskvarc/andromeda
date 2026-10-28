//! PostgreSQL implementation of the registration persistence port, with the error and outcome translation its reads and writes share.

use crate::{
    csource_registration::{registration_read, registration_write},
    store_fault::persistence_error,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{csource_change_revision::CsourceChangeRevision, registration::ContextSourceRegistration};
use andromeda_ngsi_ld_query::{
    csr::{compiled::CompiledCsrQuery, expanded_entity_specification::ExpandedEntitySpecification},
    page_window::PageWindow,
};
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use andromeda_postgresql_pool::sql_state::violates_unique_constraint;
use andromeda_registration::{
    error::RegistrationError,
    resolved_update::ResolvedCsrUpdate,
    store::{
        RegistrationStore,
        create_params::CreateCsrParams,
        expired_deletion::ExpiredDeletion,
        forwarding_outcome::ForwardingOutcome,
        mutation::RegistrationMutation,
        page::RegistrationPage,
        written_registration::WrittenRegistration,
    },
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;
use std::sync::Arc;

/// The primary key constraint on `context_source_registrations.id`, the registration identifier itself.
const PK_CONTEXT_SOURCE_REGISTRATIONS: &str = "pk_context_source_registrations";

/// PostgreSQL-backed registration persistence adapter.
///
/// Holds each registration as its `context_source_registrations` row and its
/// `registration_information` rows, the one store of its `information`, which
/// every response body is assembled from and every matching read filters on.
/// Every mutation below rewrites the rows in the same transaction as the
/// registration row, so no reader observes one without the other.
#[derive(Debug)]
pub struct PostgresRegistrationStore {
    /// Tenant-scoped PostgreSQL connection pool.
    pub(super) pool: PgPool,
    /// The tenant's attribute-name dictionary, which the information rows
    /// intern registered property and relationship names through.
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
        persistence_error::<RegistrationError>(error)
    }
}

/// Convert PostgreSQL's signed count representation to the policy-owned count.
pub(super) fn registration_count(total: i64) -> Result<u64, RegistrationError> {
    u64::try_from(total).map_err(|error| persistence_error::<RegistrationError>(error))
}

#[async_trait]
impl RegistrationStore for PostgresRegistrationStore {
    async fn delete_expired(&self, cutoff: DateTime<Utc>) -> Result<ExpiredDeletion, RegistrationError> {
        registration_write::delete_expired(self, cutoff).await
    }

    async fn find_expired(&self, cutoff: DateTime<Utc>) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        registration_read::find_expired(self, cutoff).await
    }

    async fn exists(&self, id: &Identifier) -> Result<bool, RegistrationError> {
        registration_read::exists(self, id).await
    }

    async fn create(&self, id: &Identifier, params: &CreateCsrParams) -> Result<WrittenRegistration, RegistrationError> {
        registration_write::create(self, id, params).await
    }

    async fn get(&self, id: &Identifier) -> Result<Option<ContextSourceRegistration>, RegistrationError> {
        registration_read::get(self, id).await
    }

    async fn query_compiled(&self, compiled: &CompiledCsrQuery, window: &PageWindow) -> Result<RegistrationPage, RegistrationError> {
        registration_read::query_compiled(self, compiled, window).await
    }

    async fn update(&self, id: &Identifier, values: &ResolvedCsrUpdate) -> Result<RegistrationMutation<WrittenRegistration>, RegistrationError> {
        registration_write::update(self, id, values).await
    }

    async fn find_matching(&self, entities: &[ExpandedEntitySpecification<'_>]) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        registration_read::find_matching(self, entities).await
    }

    async fn find_overlapping(
        &self,
        entity_id: Option<&Identifier>,
        attributes: &[AttributeName],
    ) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        registration_read::find_overlapping(self, entity_id, attributes).await
    }

    async fn find_all_active(&self, after: Option<&Identifier>, limit: u32) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        registration_read::find_all_active(self, after, limit).await
    }

    async fn record_forwarding_outcome(&self, id: &Identifier, outcome: ForwardingOutcome, at: DateTime<Utc>) -> Result<(), RegistrationError> {
        registration_write::record_forwarding_outcome(self, id, outcome, at).await
    }

    async fn delete(&self, id: &Identifier) -> Result<RegistrationMutation<CsourceChangeRevision>, RegistrationError> {
        registration_write::delete(self, id).await
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        csource_registration::registration_store::{PK_CONTEXT_SOURCE_REGISTRATIONS, create_error},
        migration_set::tenant::tenant_migrator,
        store_fault::persistence_error,
    };
    use andromeda_registration::error::RegistrationError;
    use std::{
        fmt::Debug,
        sync::{Arc, Mutex},
        thread::yield_now,
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

    /// Every constraint is named, so the duplicate-identifier refusal matches the declared name rather than one PostgreSQL generated.
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

    #[test]
    fn a_pool_timeout_becomes_store_saturated() {
        assert!(matches!(
            persistence_error::<RegistrationError>(sqlx::Error::PoolTimedOut),
            RegistrationError::StoreSaturated
        ));
        assert!(matches!(
            persistence_error::<RegistrationError>(sqlx::Error::RowNotFound),
            RegistrationError::Persistence(_)
        ));
    }

    #[test]
    fn a_lost_connection_becomes_store_unavailable() {
        use std::io::{Error as IoError, ErrorKind};
        assert!(matches!(
            persistence_error::<RegistrationError>(sqlx::Error::Io(IoError::from(ErrorKind::ConnectionReset))),
            RegistrationError::StoreUnavailable
        ));
        assert!(matches!(
            persistence_error::<RegistrationError>(sqlx::Error::RowNotFound),
            RegistrationError::Persistence(_)
        ));
    }

    /// How many times the callsite interest cache is rebuilt before a
    /// recording that never arrives fails the test.
    const INTEREST_REBUILD_ATTEMPTS: usize = 1_000;

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
        let locations = Arc::new(Mutex::new(Vec::new()));
        let subscriber = RecordedLocations {
            locations: Arc::clone(&locations),
        };
        // Other tests reach the same callsite on threads with no subscriber,
        // and a callsite first registered there caches that no one listens.
        // A registration still in flight on such a thread stores that answer
        // after any rebuild that ran before it finished, so the cache is
        // rebuilt until the event reaches this subscriber.
        with_default(subscriber, || {
            (0..INTEREST_REBUILD_ATTEMPTS).any(|_| {
                rebuild_interest_cache();
                let failed: Result<(), sqlx::Error> = Err(sqlx::Error::RowNotFound);
                assert!(
                    failed.map_err(|error| persistence_error::<RegistrationError>(error)).is_err(),
                    "the statement failed"
                );
                let recorded = locations.lock().is_ok_and(|locations| !locations.is_empty());
                if !recorded {
                    yield_now();
                }
                recorded
            })
        });
        let locations = locations.lock().expect("the test lock is not poisoned");
        assert!(!locations.is_empty(), "the failure was not recorded");
        assert!(locations.iter().all(|location| location.contains("registration_store.rs")), "{locations:?}");
    }
}
