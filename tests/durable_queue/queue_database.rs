//! The database the durable-queue verifications own, and the rows they seed it with.
//!
//! The database is created once from the broker's own tenant migrations and
//! then reused: creating it is idempotent, migrating an already-migrated
//! database is a no-op, and every verification empties the queue relations
//! before it seeds its own fixture.

use andromeda_context_commit::{
    job_lease::JobLease,
    lease_acknowledgement::LeaseAcknowledgement,
    subscription_key::SubscriptionKey,
    write_id::WriteId,
    write_ordinal::WriteOrdinal,
    write_record::WriteRecord,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_postgresql_attribute_instance::name_keys::{MentionedNames, NameKeys};
use andromeda_postgresql_codes::{subscription_kind::SubscriptionKindCode, subscription_status::SubscriptionStatusCode};
use andromeda_postgresql_context_store::{
    config::SynchronousCommit,
    error::ContextStoreError,
    notification_delivery::{event_insert::insert_events_in_tx, queue::NotificationDeliveryQueue, rows::NewNotificationEvent},
    pending_side_effects::{payload::PersistedBatchSideEffectJob, repo::PendingSideEffectsRepository},
    tenant_database::{DatabaseUrl, create_database_if_missing},
    tenant_migration::migrate_tenant_db,
};
use andromeda_postgresql_name_dictionary::{name_cache::NameCache, name_cache_config::NameCacheConfig, name_lookup::NameLookup};
use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, dictionary::resolved_names::ResolvedNames};
use andromeda_tenancy::database_name::TenantDatabaseName;
use andromeda_test_harness::{configuration::TestRunConfiguration, database_reset::reset_owned_tenant, dictionary_seed::attribute_name_key};
use chrono::Utc;
use serde_json::json;
use sqlx::{PgPool, Postgres, Transaction, postgres::PgPoolOptions};
use std::sync::Arc;

/// Database these verifications own outright.
const QUEUE_DATABASE: &str = "andromeda_verification_durable_queue";

/// Open the suite's own tenant database with its queue relations empty.
///
/// The reset is the harness-owned tenant reset: `subscriptions` is emptied with
/// the queue because a delivery references a Subscription, and the surrogate key
/// it carries is restarted with it.
pub async fn queue_database() -> PgPool {
    let configuration = TestRunConfiguration::from_environment().expect("integration test configuration");
    // clone: `DatabaseUrl` owns the endpoint it derives per-database URLs from.
    let base = DatabaseUrl::new(configuration.database().url().clone());
    let name = TenantDatabaseName::from(QUEUE_DATABASE.to_owned());
    create_database_if_missing(&base, &name, SynchronousCommit::Off.as_postgres_setting())
        .await
        .expect("queue verification database");
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(base.with_database_name(&name).as_str())
        .await
        .expect("queue verification database connection");
    migrate_tenant_db(&pool).await.expect("queue verification database migrations");
    reset_owned_tenant(&pool).await.expect("empty queue relations");
    pool
}

/// The tenant's live branch: the default schema, addressed unqualified.
#[must_use]
pub const fn live_target() -> ContextTarget {
    ContextTarget::Live
}

/// Seed one Subscription and return the surrogate key its deliveries carry.
///
/// Only the columns the schema requires are written: the queue verifications
/// address a Subscription by its key and its external identifier, and nothing
/// here evaluates a selector. The live branch's rows live in the default schema,
/// so the insert names no branch.
pub async fn subscription(pool: &PgPool, id: &str) -> i64 {
    let notification = json!({
        "endpoint": {"uri": "http://127.0.0.1:8085/notify", "accept": "application/json"},
        "format": "normalized"
    });
    sqlx::query_scalar("INSERT INTO subscriptions (id, kind, status, notification) VALUES ($1, $2, $3, $4) RETURNING subscription_key")
        .bind(id)
        .bind(SubscriptionKindCode::Entity.code())
        .bind(SubscriptionStatusCode::Active.code())
        .bind(notification)
        .fetch_one(pool)
        .await
        .expect("seeded subscription")
}

/// The typed surrogate key of a seeded Subscription, as a matcher would carry it.
pub fn subscription_key(stored: i64) -> SubscriptionKey {
    SubscriptionKey::try_from(stored).expect("a seeded subscription key is positive")
}

/// Seed one dictionary entry and return the four-byte key an event stores.
pub async fn attribute_name(pool: &PgPool, name: &str) -> AttributeNameKey {
    AttributeNameKey::from(attribute_name_key(pool, name).await.expect("seeded attribute name"))
}

/// The tenant's two name dictionaries, cached as a broker caches them.
pub fn dictionaries(pool: &PgPool) -> (Arc<NameCache<AttributeName>>, Arc<NameCache<EntityTypeName>>) {
    let config = NameCacheConfig::default();
    (Arc::new(NameCache::new(pool, &config)), Arc::new(NameCache::new(pool, &config)))
}

/// The queue a broker serving the verification database claims from.
pub fn delivery_queue(pool: &PgPool) -> NotificationDeliveryQueue {
    let (attribute_names, entity_type_names) = dictionaries(pool);
    NotificationDeliveryQueue::new(pool, attribute_names, entity_type_names)
}

/// Write one write's notification events and the deliveries they owe, minting
/// the names their states hold in the same transaction, as the commit does.
///
/// The failure is returned rather than asserted away: a replayed write is
/// expected to be refused by the queue's own write-identity constraint, and
/// that refusal is the assertion.
pub async fn record_events(pool: &PgPool, record: &WriteRecord, events: &[NewNotificationEvent<'_>]) -> Result<(), ContextStoreError> {
    let target = live_target();
    let mut transaction = pool.begin().await?;
    let keys = state_keys(pool, &mut transaction, events).await?;
    insert_events_in_tx(&mut transaction, &target, record, events, &keys).await?;
    transaction.commit().await?;
    Ok(())
}

/// The keys of every name the states of `events` hold, minted in `transaction`.
pub async fn state_keys(
    pool: &PgPool,
    transaction: &mut Transaction<'_, Postgres>,
    events: &[NewNotificationEvent<'_>],
) -> Result<NameKeys, ContextStoreError> {
    let mut mentioned = MentionedNames::default();
    for state in events.iter().flat_map(|event| event.entity_snapshot.into_iter().chain(event.previous_snapshot)) {
        mentioned.collect(state.attributes());
        mentioned.types.extend(state.types().as_slice().iter().cloned()); // clone: the dictionary request owns the names it resolves.
    }
    let (attribute_names, entity_type_names) = dictionaries(pool);
    let attributes = attribute_names
        .resolve_or_create(transaction, &mentioned.attributes)
        .await
        .map_err(ContextStoreError::NameLookup)?;
    let types = entity_type_names
        .resolve_or_create(transaction, &mentioned.types)
        .await
        .map_err(ContextStoreError::NameLookup)?;
    Ok(NameKeys::new(ResolvedNames::from_pairs(attributes), ResolvedNames::from_pairs(types)))
}

/// Count the deliveries the queue currently holds for one Subscription key.
pub async fn deliveries_of(pool: &PgPool, subscription_key: i64) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM notification_deliveries WHERE subscription_key = $1")
        .bind(subscription_key)
        .fetch_one(pool)
        .await
        .expect("delivery count")
}

/// Count the events one write left behind.
pub async fn events_of(pool: &PgPool, record: &WriteRecord) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM notification_events WHERE write_id = $1")
        .bind(record.write_id().as_uuid())
        .fetch_one(pool)
        .await
        .expect("event count")
}

/// Enqueue one decodable side-effect job through the committer's own enqueue
/// statement, returning the identity of the write it belongs to.
pub async fn enqueue_side_effect(pool: &PgPool) -> WriteId {
    let record = WriteRecord::mint(Utc::now());
    let payload = PersistedBatchSideEffectJob {
        temporal_entities: Vec::new(),
        change_events: Vec::new(),
    };
    let mut transaction = pool.begin().await.expect("enqueue transaction");
    PendingSideEffectsRepository::enqueue_in_tx(&mut transaction, &record, WriteOrdinal::at(0), &payload)
        .await
        .expect("enqueue the job");
    transaction.commit().await.expect("commit the enqueue");
    record.write_id()
}

/// Settle one side-effect job under `lease` in a transaction of its own, the
/// way a replay commit settles it inside its own transaction.
pub async fn settle_side_effect(pool: &PgPool, lease: JobLease) -> LeaseAcknowledgement {
    let mut transaction = pool.begin().await.expect("settle transaction");
    let settled = PendingSideEffectsRepository::settle_in_tx(&mut transaction, lease)
        .await
        .expect("settle the job");
    transaction.commit().await.expect("commit the settlement");
    settled
}
