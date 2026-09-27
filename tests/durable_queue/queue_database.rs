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
use andromeda_postgresql_codes::{subscription_kind::SubscriptionKindCode, subscription_status::SubscriptionStatusCode};
use andromeda_postgresql_context_store::{
    config::SynchronousCommit,
    error::ContextStoreError,
    notification_delivery::{event_insert::insert_events_in_tx, rows::NewNotificationEvent},
    pending_side_effects::{payload::PersistedBatchSideEffectJob, repo::PendingSideEffectsRepository},
    tenant_database::{DatabaseUrl, create_database_if_missing},
    tenant_migration::migrate_tenant_db,
};
use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
use andromeda_tenancy::database_name::TenantDatabaseName;
use andromeda_test_harness::{configuration::TestRunConfiguration, database_reset::reset_owned_tenant};
use chrono::Utc;
use serde_json::json;
use sqlx::{PgPool, postgres::PgPoolOptions};

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
    let key: i32 = sqlx::query_scalar(
        "INSERT INTO attribute_names (name) VALUES ($1) ON CONFLICT ON CONSTRAINT uq_attribute_names_name DO UPDATE SET name = EXCLUDED.name RETURNING attribute_name_key",
    )
    .bind(name)
    .fetch_one(pool)
    .await
    .expect("seeded attribute name");
    AttributeNameKey::from(key)
}

/// Write one write's notification events and the deliveries they owe.
///
/// The failure is returned rather than asserted away: a replayed write is
/// expected to be refused by the queue's own write-identity constraint, and
/// that refusal is the assertion.
pub async fn record_events(pool: &PgPool, record: &WriteRecord, events: &[NewNotificationEvent<'_>]) -> Result<(), ContextStoreError> {
    let target = live_target();
    let mut transaction = pool.begin().await?;
    insert_events_in_tx(&mut transaction, &target, record, events).await?;
    transaction.commit().await?;
    Ok(())
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
