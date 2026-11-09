//! Reading a tenant's database for the earliest instant it has work due.
//!
//! Implements the tenancy crate's [`PendingWorkProbe`] port, and answers a
//! resident tenant's background pacer through [`earliest_obligation`]. A tenant
//! no request is touching still owes background work, namely durable-delivery
//! retries, snapshot status and preparation, pending side effects, temporal
//! append intents, every `expiresAt` sweep, clause-5.8.6 and clause-5.11.7
//! periodic notifications, the clause-5.13.1 invalidation of every
//! `"Cached"` `@context` whose expiration time has passed, and the clause
//! 5.8.1.4 copy placements cut off before their source answered. The probe answers
//! the single earliest instant any of those falls due, over a transient
//! one-connection pool so it adds no permanent backend and is safe through a
//! transaction pooler. The pool reaches an established tenant over its request
//! URL and a provisional one directly ([`TenantProbeUrls`]).
//!
//! The queue and expiry sources are one `SELECT LEAST(...)` of index-backed
//! scalar subqueries, each served by the same partial or expression index the
//! owning worker's claim uses, plus one statement per published Snapshot
//! branch for its delivery queue. The periodic-notification source is computed from
//! each active subscription's grid anchor, interval and last answered sweep
//! instant with the shared [`next_interval_due`] grid, so the probe and the
//! delivery drain agree on when a subscription is next due.

use crate::{
    error::ContextStoreError,
    migration_set::branch::current_branch_version,
    notification_delivery::branches::{drainable_targets, is_reaped_relation},
    pending_work::due_sources::{branch_delivery_due_select, least_due_select},
    store_fault::persistence_failure,
    subscription_store::{delivery_state::interval_anchor_select, periodic_due::unexpired_at, status_code::status_is},
    tenant_migration::{TenantSchemaState, tenant_schema_state},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use andromeda_ngsi_ld_model::subscription::SubscriptionStatus;
use andromeda_postgresql_pool::{pending_work_failure::pending_work_failure, pool_options::configure_pool_options, tenant_probe_urls::TenantProbeUrls};
use andromeda_postgresql_schema::subscriptions::Subscriptions;
use andromeda_postgresql_seaquery::{clock::now, statement::Statement};
use andromeda_subscription::interval_schedule::{interval_duration, next_interval_due};
use andromeda_tenancy::{
    pending_work::{PendingWork, PendingWorkError, PendingWorkProbe},
    tenant_existence::TenantExistence,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sea_query::{Expr, ExprTrait, SelectStatement};
use std::time::{Duration, SystemTime};

/// The queue and expiry sources of the earliest obligation, as one statement.
mod due_sources;

/// Reads a tenant's own database for the earliest instant it has work due.
///
/// Holds the bases each tenant's connection string is derived from, and the
/// acquire deadline the transient probe pool gives up after.
pub struct PostgresPendingWorkProbe {
    /// The bases every per-tenant probe connection is derived from.
    urls: TenantProbeUrls,
    /// How long the transient probe pool waits for a connection before it treats
    /// the backend as saturated.
    acquire_timeout: Duration,
}

impl PostgresPendingWorkProbe {
    /// A probe over the given bases and acquire deadline.
    #[must_use]
    pub const fn new(urls: TenantProbeUrls, acquire_timeout: Duration) -> PostgresPendingWorkProbe {
        PostgresPendingWorkProbe { urls, acquire_timeout }
    }
}

#[async_trait]
impl PendingWorkProbe for PostgresPendingWorkProbe {
    async fn earliest_pending_work(&self, database: &TenantDatabaseName, existence: TenantExistence) -> Result<PendingWork, PendingWorkError> {
        let url = self.urls.tenant_url(database, existence);
        let pool = configure_pool_options(1, self.acquire_timeout)
            .connect(url.as_str())
            .await
            .map_err(|error| pending_work_failure(&error, database))?;

        let earliest = Self::earliest_over(&pool, database).await;
        pool.close().await;
        earliest
    }
}

impl PostgresPendingWorkProbe {
    /// Read the earliest obligation over the transient pool once the schema
    /// contract shows the database migrated, classifying a failure the way the
    /// wake loop needs it.
    ///
    /// The schema state is read first because a provisioning commits the
    /// tenant's row before it migrates the database: until the migrator stamps
    /// the contract, the relations the obligation read names are missing or
    /// still being created, and the read would fail or wait on the migration's
    /// locks.
    async fn earliest_over(pool: &sqlx::PgPool, database: &TenantDatabaseName) -> Result<PendingWork, PendingWorkError> {
        let state = tenant_schema_state(pool).await.map_err(|error| Self::classify_store_error(error, database))?;
        readable(state, database)?;
        earliest_obligation(pool)
            .await
            .map(|due| due.map_or(PendingWork::None, PendingWork::DueAt))
            .map_err(|error| Self::classify_store_error(error, database))
    }

    /// Classify a store failure the probe hit as saturation, a gone database,
    /// or a genuine fault.
    fn classify_store_error(error: ContextStoreError, database: &TenantDatabaseName) -> PendingWorkError {
        match error {
            ContextStoreError::Saturated => PendingWorkError::Saturated {
                database: database.clone(), // clone: the error outlives this borrow
            },
            ContextStoreError::Database(error) => pending_work_failure(&error, database),
            error @ (ContextStoreError::SchemaContract(_)
            | ContextStoreError::UniqueViolation(_)
            | ContextStoreError::InvalidRegex(_)
            | ContextStoreError::Encoding(_)
            | ContextStoreError::Collation(_)
            | ContextStoreError::CopyField(_)
            | ContextStoreError::Render(_)
            | ContextStoreError::NameLookup(_)
            | ContextStoreError::EntityTypes(_)
            | ContextStoreError::ContextMapper(_)
            | ContextStoreError::Migration(_)
            | ContextStoreError::BranchVersion(_)
            | ContextStoreError::StatementBuild(_)
            | ContextStoreError::ChangeEventRecord(_)
            | ContextStoreError::StateRecord(_)
            | ContextStoreError::OrdinalOutOfRange(_)
            | ContextStoreError::StoredRegistration(_)
            | ContextStoreError::StoredSubscription(_)
            | ContextStoreError::StoredSnapshot(_)
            | ContextStoreError::UnencodableSnapshot(_)
            | ContextStoreError::StoredEntity(_)
            | ContextStoreError::StoredEntityMap(_)
            | ContextStoreError::StoredTenantStatus(_)
            | ContextStoreError::CatalogueIdentityGeneration(_)
            | ContextStoreError::Invariant(_)
            | ContextStoreError::DatabaseUrlUnnamed) => PendingWorkError::ProbeFailed {
                database: database.clone(), // clone: the error outlives this borrow
                cause: persistence_failure(&error),
            },
        }
    }
}

/// Whether the probe may read obligations from a database whose schema is in
/// `state`: only a schema at this broker's contract holds the relations it
/// reads.
fn readable(state: TenantSchemaState, database: &TenantDatabaseName) -> Result<(), PendingWorkError> {
    match state {
        TenantSchemaState::Current => Ok(()),
        TenantSchemaState::NeedsMigration => Err(PendingWorkError::Unmigrated {
            database: database.clone(), // clone: the error outlives this borrow
        }),
    }
}

/// The earliest instant any background obligation of the database behind
/// `pool` falls due (a durable-queue item, a preparing Snapshot, an
/// `expiresAt` of any kind, a clause-5.8.6 periodic notification, or the end
/// of an unsettled subscription copy claim's lease), or `None` when nothing is
/// outstanding.
///
/// One index-backed `SELECT LEAST(...)`, one delivery-queue read per published
/// Snapshot branch, and the periodic-notification anchors.
/// It serves both the wake loop's probe of a dormant tenant, over a transient
/// pool, and a resident tenant pacing its own sweeps, over its serving pool.
///
/// # Errors
///
/// Returns [`ContextStoreError::BranchVersion`] when the embedded branch set has
/// no version, and the classified database failure of either read.
pub async fn earliest_obligation(pool: &sqlx::PgPool) -> Result<Option<SystemTime>, ContextStoreError> {
    let queue_due = earliest_queue_due(pool).await?;

    let anchors: Vec<(DateTime<Utc>, f64, Option<DateTime<Utc>>)> = interval_probe_select().to_query_as().fetch_all(pool).await?;

    Ok([queue_due, earliest_interval_due(anchors)].into_iter().flatten().min().map(SystemTime::from))
}

/// The earliest due instant across the queue and expiry sources, reading the
/// delivery queue of every branch the delivery drain sweeps.
///
/// The live branch's sources are one statement whose text never changes, and
/// each published Snapshot branch's delivery queue is a statement of its own,
/// so a connection plans each once however the Snapshot set churns. A Snapshot
/// reaped between the branch enumeration and its read has dropped its queue
/// with its schema, so the missing relation means that branch owes nothing.
async fn earliest_queue_due(pool: &sqlx::PgPool) -> Result<Option<DateTime<Utc>>, ContextStoreError> {
    let mut earliest: Option<DateTime<Utc>> = least_due_select(current_branch_version()?).to_query_scalar().fetch_one(pool).await?;
    for branch in drainable_targets(pool).await? {
        match branch {
            ContextTarget::Live => {}
            ContextTarget::Snapshot(_) => match branch_delivery_due_select(&branch).to_query_scalar().fetch_one(pool).await {
                Ok(due) => earliest = [earliest, due].into_iter().flatten().min(),
                Err(error) if is_reaped_relation(&error) => {}
                Err(error) => return Err(error.into()),
            },
        }
    }
    Ok(earliest)
}

/// The earliest instant any periodic subscription owes a notification.
///
/// A subscription owes the first grid point after the sweep instant it last
/// answered at, or its first grid point when it never has. When the tenant has
/// been dormant across that point it is in the past, so the probe reports the
/// subscription as due now and the tenant is woken to deliver, rather than
/// the next grid point after now, which would always be a period in the future
/// and would never wake an overdue subscription. The sweep records the instant
/// it answers at, so the next probe sees the following grid point.
fn earliest_interval_due(anchors: Vec<(DateTime<Utc>, f64, Option<DateTime<Utc>>)>) -> Option<DateTime<Utc>> {
    anchors
        .into_iter()
        .filter_map(|(anchor, seconds, tick)| interval_duration(seconds).and_then(|interval| next_interval_due(anchor, interval, tick.unwrap_or(anchor))))
        .min()
}

/// The active periodic subscriptions' grid anchors and intervals, for entity
/// subscriptions (clause 5.8.6) and context source subscriptions (clause
/// 5.11.7) alike, which share the table, so the caller can compute the
/// soonest next-due with the shared grid.
///
/// Only an active subscription still in force is owed a periodic notification:
/// a paused one sends nothing, and once `expiresAt` has passed "notifications
/// will no longer be sent" (clause 5.8.1.4), even before the expiry sweep has
/// marked it expired. Counting either would report the tenant due forever.
fn interval_probe_select() -> SelectStatement {
    let mut select = interval_anchor_select(&ContextTarget::Live);
    select
        .and_where(Expr::col((Subscriptions::Table, Subscriptions::TimeInterval)).is_not_null())
        .and_where(Expr::col((Subscriptions::Table, Subscriptions::TimeInterval)).gt(0.0))
        .and_where(status_is(SubscriptionStatus::Active))
        .cond_where(unexpired_at(now()));
    select
}

#[cfg(test)]
mod tests {
    use crate::{
        error::ContextStoreError,
        pending_work::{PostgresPendingWorkProbe, earliest_interval_due, interval_probe_select, readable},
        tenant_migration::TenantSchemaState,
    };
    use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
    use andromeda_postgresql_pool::fake_database_error::FakeDatabaseError;
    use andromeda_tenancy::pending_work::PendingWorkError;
    use chrono::{Duration, Utc};
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn a_tenant_whose_migrations_are_running_is_not_read_for_obligations() {
        let database = TenantDatabaseName::from("andromeda_tenant_migrating".to_string());
        assert!(matches!(
            readable(TenantSchemaState::NeedsMigration, &database),
            Err(PendingWorkError::Unmigrated { .. })
        ));
        assert!(readable(TenantSchemaState::Current, &database).is_ok());
    }

    #[test]
    fn a_schema_state_read_failure_is_classified_like_the_obligation_read() {
        let database = TenantDatabaseName::from("andromeda_tenant_acme".to_string());
        assert!(matches!(
            PostgresPendingWorkProbe::classify_store_error(ContextStoreError::Saturated, &database),
            PendingWorkError::Saturated { .. }
        ));
        assert!(matches!(
            PostgresPendingWorkProbe::classify_store_error(
                ContextStoreError::Database(FakeDatabaseError::new("no database", "3D000").into_sqlx()),
                &database
            ),
            PendingWorkError::DatabaseGone { .. }
        ));
        assert!(matches!(
            PostgresPendingWorkProbe::classify_store_error(ContextStoreError::DatabaseUrlUnnamed, &database),
            PendingWorkError::ProbeFailed { .. }
        ));
    }

    #[test]
    fn the_probe_reports_an_overdue_interval_as_due_now() {
        let now = Utc::now();
        // Created 100 s ago on a 5 s grid and last answered 50 s ago, the
        // subscription owes the grid point 55 s after its creation.
        let created = now - Duration::seconds(100);
        let answered = now - Duration::seconds(50);
        let due = earliest_interval_due(vec![(created, 5.0, Some(answered))]);
        assert_eq!(due, Some(created + Duration::seconds(55)));
        assert!(
            due.is_some_and(|instant| instant <= now),
            "an overdue interval subscription is due now, got {due:?}"
        );

        let never_answered = earliest_interval_due(vec![(created, 5.0, None)]);
        assert_eq!(
            never_answered,
            Some(created + Duration::seconds(5)),
            "a subscription never answered owes its first grid point"
        );

        let ahead = earliest_interval_due(vec![(now, 5.0, None)]);
        assert!(
            ahead.is_some_and(|instant| instant > now),
            "a fresh interval subscription is due a period ahead, got {ahead:?}"
        );

        assert_eq!(
            earliest_interval_due(vec![(now, 0.0, None)]),
            None,
            "a non-positive interval yields no due instant"
        );
    }

    /// Entity and context source subscriptions share the table, so one source
    /// serves both kinds of periodic notification.
    #[test]
    fn the_interval_source_reads_every_kind_of_subscription() {
        let interval_sql = interval_probe_select().to_string(PostgresQueryBuilder);
        assert!(!interval_sql.contains("\"kind\""), "no kind filter narrows the periodic source: {interval_sql}");
    }

    /// Neither a paused subscription nor one past its `expiresAt` is owed a
    /// periodic notification, so neither keeps the tenant due.
    #[test]
    fn the_interval_source_reads_active_subscriptions_still_in_force() {
        let interval_sql = interval_probe_select().to_string(PostgresQueryBuilder);
        assert!(
            interval_sql.contains("\"subscriptions\".\"status\" = 1"),
            "the interval source reads active subscriptions only: {interval_sql}"
        );
        assert!(
            interval_sql.contains("(\"subscriptions\".\"expires_at\" IS NULL OR \"subscriptions\".\"expires_at\" > NOW())"),
            "the interval source reads subscriptions still in force only: {interval_sql}"
        );
        assert!(
            interval_sql.contains("\"time_interval\""),
            "the interval source reads the period: {interval_sql}"
        );
    }
}
