//! Reading a tenant's database for the earliest instant it has work due.
//!
//! Implements the tenancy crate's [`PendingWorkProbe`] port, and answers a
//! resident tenant's background pacer through [`earliest_obligation`]. A tenant
//! no request is touching still owes background work -- durable-delivery
//! retries, snapshot status and preparation, pending side effects, temporal
//! append intents, every `expiresAt` sweep, clause-5.8.6 and clause-5.11.7
//! periodic notifications, and the clause-5.13.1 invalidation of every
//! `"Cached"` `@context` whose expiration time has passed. The probe answers
//! the single earliest instant any of those falls due, over a transient
//! one-connection pool on the tenant's request URL so it adds no permanent
//! backend and is safe through a transaction pooler.
//!
//! The queue and expiry sources are one `SELECT LEAST(...)` of index-backed
//! scalar subqueries, each served by the same partial or expression index the
//! owning worker's claim uses. The periodic-notification source is computed from
//! each active subscription's stored grid anchor and interval with the shared
//! [`next_interval_due`] grid, so the probe and the delivery drain agree on when
//! a subscription is next due.

use crate::{
    error::ContextStoreError,
    migration_set::branch::{BranchSchemaVersion, current_branch_version},
    notification_delivery::branches::{drainable_targets, is_reaped_branch},
    snapshot::{branch_version::branch_is_current, reap::orphan_branch_keys_statement, status_code::stored_status as stored_snapshot_status},
    store_fault::persistence_failure,
    subscription_store::{
        delivery_state::interval_anchor_select,
        periodic_due::unexpired_at,
        status_code::{status_is, status_is_not},
    },
    tenant_database::DatabaseUrl,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_model::{snapshot::SnapshotStatus, subscription::SubscriptionStatus};
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_codes::jsonld_context_kind::JsonldContextKindCode;
use andromeda_postgresql_pool::{pool_options::configure_pool_options, saturation::is_pool_saturated};
use andromeda_postgresql_schema::{
    csr::Csr,
    current::Entities,
    entity_map::EntityMaps,
    jsonld::JsonldContexts,
    notification::NotificationDeliveries,
    queue::{PendingSideEffects, TemporalAppendIntents},
    snapshot::{SnapshotNotificationOutbox, SnapshotPreparation, Snapshots},
    subscriptions::Subscriptions,
};
use andromeda_postgresql_seaquery::{clock::now, queue::claimable_at};
use andromeda_subscription::interval_schedule::interval_duration;
use andromeda_tenancy::{
    database_name::TenantDatabaseName,
    pending_work::{PendingWork, PendingWorkError, PendingWorkProbe},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sea_query::{Alias, Expr, ExprTrait, Func, IntoColumnRef, IntoTableRef, PostgresQueryBuilder, Query, QueryStatementBuilder, SelectStatement, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::error::DatabaseError;
use std::time::{Duration, SystemTime};

/// Reads a tenant's own database for the earliest instant it has work due.
///
/// Holds the request base URL -- the pooler when one is configured -- from which
/// each tenant's connection string is derived, and the acquire deadline the
/// transient probe pool gives up after.
pub struct PostgresPendingWorkProbe {
    /// The request base URL every per-tenant probe connection is derived from.
    request_url: DatabaseUrl,
    /// How long the transient probe pool waits for a connection before it treats
    /// the backend as saturated.
    acquire_timeout: Duration,
}

impl PostgresPendingWorkProbe {
    /// A probe over the given request base URL and acquire deadline.
    #[must_use]
    pub const fn new(request_url: DatabaseUrl, acquire_timeout: Duration) -> PostgresPendingWorkProbe {
        PostgresPendingWorkProbe { request_url, acquire_timeout }
    }

    /// Classify a `sqlx` failure the probe hit as saturation, a gone database,
    /// or a genuine fault.
    fn classify(error: &sqlx::Error, database: &TenantDatabaseName) -> PendingWorkError {
        if is_pool_saturated(error) {
            PendingWorkError::Saturated { database: database.clone() } // clone: the error outlives this borrow
        } else if code_is_undefined_database(error.as_database_error().and_then(DatabaseError::code).as_deref()) {
            PendingWorkError::DatabaseGone { database: database.clone() } // clone: the error outlives this borrow
        } else {
            PendingWorkError::ProbeFailed {
                database: database.clone(), // clone: the error outlives this borrow
                cause: persistence_failure(&error),
            }
        }
    }
}

/// Whether a SQLSTATE is `3D000` (`invalid_catalog_name` -- the database the
/// probe tried to open does not exist), which a deprovisioned tenant's stale
/// wake-schedule row produces on connect.
fn code_is_undefined_database(code: Option<&str>) -> bool {
    code == Some("3D000")
}

#[async_trait]
impl PendingWorkProbe for PostgresPendingWorkProbe {
    async fn earliest_pending_work(&self, database: &TenantDatabaseName) -> Result<PendingWork, PendingWorkError> {
        let url = self.request_url.with_database_name(database);
        let pool = configure_pool_options(1, self.acquire_timeout)
            .connect(url.as_str())
            .await
            .map_err(|error| Self::classify(&error, database))?;

        let earliest = self.earliest_over(&pool, database).await;
        pool.close().await;
        earliest
    }
}

impl PostgresPendingWorkProbe {
    /// Read the earliest obligation over the transient pool, classifying a
    /// failure the way the wake loop needs it.
    async fn earliest_over(&self, pool: &sqlx::PgPool, database: &TenantDatabaseName) -> Result<PendingWork, PendingWorkError> {
        match earliest_obligation(pool).await {
            Ok(due) => Ok(due.map_or(PendingWork::None, PendingWork::DueAt)),
            Err(ContextStoreError::Saturated) => Err(PendingWorkError::Saturated {
                database: database.clone(), // clone: the error outlives this borrow
            }),
            Err(ContextStoreError::Database(error)) => Err(Self::classify(&error, database)),
            Err(
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
                | ContextStoreError::StoredEntity(_)
                | ContextStoreError::StoredEntityMap(_)
                | ContextStoreError::CatalogueIdentityGeneration(_)
                | ContextStoreError::Invariant(_)
                | ContextStoreError::DatabaseUrlUnnamed),
            ) => Err(PendingWorkError::ProbeFailed {
                database: database.clone(), // clone: the error outlives this borrow
                cause: persistence_failure(&error),
            }),
        }
    }
}

/// The earliest instant any background obligation of the database behind
/// `pool` falls due -- a durable-queue item, a preparing Snapshot, an
/// `expiresAt` of any kind, or a clause-5.8.6 periodic notification -- or
/// `None` when nothing is outstanding.
///
/// One index-backed `SELECT LEAST(...)` plus the periodic-notification anchors.
/// It serves both the wake loop's probe of a dormant tenant, over a transient
/// pool, and a resident tenant pacing its own sweeps, over its serving pool.
///
/// # Errors
///
/// Returns [`ContextStoreError::BranchVersion`] when the embedded branch set has
/// no version, and the classified database failure of either read.
pub async fn earliest_obligation(pool: &sqlx::PgPool) -> Result<Option<SystemTime>, ContextStoreError> {
    let queue_due = match earliest_queue_due(pool).await {
        // A Snapshot reaped between the branch enumeration and the read drops
        // the schema the read names; enumerating again leaves it out.
        Err(error) if is_reaped_branch(&error) => earliest_queue_due(pool).await?,
        other => other?,
    };

    let (interval_sql, interval_values) = interval_probe_select().build_sqlx(PostgresQueryBuilder);
    let anchors: Vec<(DateTime<Utc>, f64)> = sqlx::query_as_with(sqlx::AssertSqlSafe(interval_sql.as_str()), interval_values)
        .fetch_all(pool)
        .await?;

    Ok([queue_due, earliest_interval_due(anchors)].into_iter().flatten().min().map(SystemTime::from))
}

/// The earliest due instant across the queue and expiry sources, reading the
/// delivery queue of every branch the delivery drain sweeps.
async fn earliest_queue_due(pool: &sqlx::PgPool) -> Result<Option<DateTime<Utc>>, ContextStoreError> {
    let branches = drainable_targets(pool).await?;
    let (sql, values) = least_due_select(current_branch_version()?, &branches).build_sqlx(PostgresQueryBuilder);
    Ok(sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(pool).await?)
}

/// The earliest instant any periodic subscription owes a notification.
///
/// A subscription's earliest owed notification is the first grid point after its
/// last delivery, `anchor + interval`. When the tenant has been dormant across
/// missed periods this is in the past, so the probe reports the subscription as
/// due now and the tenant is woken to deliver -- rather than the next grid point
/// after now, which would always be a period in the future and would never wake
/// an overdue subscription. The delivery drain advances the anchor once it runs,
/// so the next probe sees the following grid point.
fn earliest_interval_due(anchors: Vec<(DateTime<Utc>, f64)>) -> Option<DateTime<Utc>> {
    anchors
        .into_iter()
        .filter_map(|(anchor, seconds)| interval_duration(seconds).and_then(|interval| anchor.checked_add_signed(interval)))
        .min()
}

/// `SELECT LEAST(...)` of the earliest due instant in each queue and expiry
/// source, a single row whose one nullable column is the soonest, or NULL when
/// nothing is due.
///
/// Every branch in `branches` contributes its delivery queue: a delivery lives
/// in the queue of the branch whose subscription matched, and the delivery
/// drain claims from each published Snapshot branch as from the live one, so a
/// Snapshot-branch retry is an obligation of the tenant like any other.
fn least_due_select(current: BranchSchemaVersion, branches: &[ContextTarget]) -> SelectStatement {
    let deliveries = branches.iter().map(|branch| {
        claimable_min(
            table_of(branch, NotificationDeliveries::Table),
            NotificationDeliveries::NextAttemptAt,
            NotificationDeliveries::LeaseUntil,
            unfinished_delivery(),
        )
    });
    let sources: Vec<SimpleExpr> = deliveries
        .chain([
            claimable_min(
                SnapshotNotificationOutbox::Table,
                SnapshotNotificationOutbox::NextAttemptAt,
                SnapshotNotificationOutbox::LeaseUntil,
                unfinished_snapshot_notification(),
            ),
            claimable_min(
                PendingSideEffects::Table,
                PendingSideEffects::NextAttemptAt,
                PendingSideEffects::LeaseUntil,
                unfinished_side_effect(),
            ),
            claimable_min_all(
                TemporalAppendIntents::Table,
                TemporalAppendIntents::NextAttemptAt,
                TemporalAppendIntents::LeaseUntil,
            ),
            preparing_min(current),
            expiry_min(Entities::Table, Entities::ExpiresAt),
            expiry_min(Entities::Table, Entities::MinAttributeExpiresAt),
            subscription_expiry_min(),
            expiry_min(Csr::Table, Csr::ExpiresAt),
            earliest_min(Snapshots::Table, Snapshots::ExpiresAt),
            orphan_branch_due(),
            earliest_min(EntityMaps::Table, EntityMaps::ExpiresAt),
            cached_context_expiry_min(),
        ])
        .collect();
    Query::select().expr_as(Func::least(sources), Alias::new("due")).to_owned()
}

/// A scalar subquery of the earliest claimable instant of a queue's unfinished
/// rows: `(SELECT MIN(GREATEST(next_attempt_at, COALESCE(lease_until, ...))) ...)`.
fn claimable_min(table: impl IntoTableRef, next_attempt: impl IntoColumnRef, lease_until: impl IntoColumnRef, unfinished: SimpleExpr) -> SimpleExpr {
    let mut select = Query::select();
    select
        .expr(Func::min(claimable_at(Expr::col(next_attempt), Expr::col(lease_until))))
        .from(table)
        .and_where(unfinished);
    scalar(select)
}

/// A scalar subquery of the earliest claimable instant across every row of a
/// queue whose rows are removed once applied, so all are outstanding.
fn claimable_min_all(table: impl IntoTableRef, next_attempt: impl IntoColumnRef, lease_until: impl IntoColumnRef) -> SimpleExpr {
    let mut select = Query::select();
    select
        .expr(Func::min(claimable_at(Expr::col(next_attempt), Expr::col(lease_until))))
        .from(table);
    scalar(select)
}

/// A scalar subquery of when the earliest still-preparing snapshot next needs a
/// preparation tick: its lease expiry, or now when it holds no lease.
///
/// A quarantined snapshot is left out: the claim never takes one, so counting
/// it would report the tenant due forever and wake it on every pass.
fn preparing_min(current: BranchSchemaVersion) -> SimpleExpr {
    let mut select = Query::select();
    select
        .expr(Func::min(Func::coalesce([Expr::col(SnapshotPreparation::LeaseUntil), now()])))
        .from(SnapshotPreparation::Table)
        .and_where(Expr::col(SnapshotPreparation::Status).eq(stored_snapshot_status(SnapshotStatus::Preparing).code()))
        .and_where(branch_is_current(current));
    scalar(select)
}

/// A scalar subquery of the earliest non-null `expires_at` in a table.
fn expiry_min(table: impl IntoTableRef, column: impl IntoColumnRef + Copy) -> SimpleExpr {
    let mut select = Query::select();
    select.expr(Func::min(Expr::col(column))).from(table).and_where(Expr::col(column).is_not_null());
    scalar(select)
}

/// A scalar subquery that is now while a branch schema outlives its
/// Snapshot's control record, so the expiry sweep that drops it is due. A
/// Snapshot deleted while PostgreSQL maintenance held its branch lock leaves
/// such a schema, and no control record is left to carry an expiry.
fn orphan_branch_due() -> SimpleExpr {
    let mut select = Query::select();
    select.expr(now()).and_where(Expr::exists(orphan_branch_keys_statement()));
    scalar(select)
}

/// A scalar subquery of the earliest `expires_at` in a table whose column is
/// always present, so no null filter is needed.
fn earliest_min(table: impl IntoTableRef, column: impl IntoColumnRef + Copy) -> SimpleExpr {
    let mut select = Query::select();
    select.expr(Func::min(Expr::col(column))).from(table);
    scalar(select)
}

/// A scalar subquery of the earliest expiration time of a `"Cached"` `@context`
/// (clause 5.13.1): the instant the periodic invalidation next has a row to
/// reclaim. Hosted contexts never expire and are left out.
fn cached_context_expiry_min() -> SimpleExpr {
    let mut select = Query::select();
    select
        .expr(Func::min(Expr::col(JsonldContexts::ExpiresAt)))
        .from(JsonldContexts::Table)
        .and_where(Expr::col(JsonldContexts::Kind).eq(JsonldContextKindCode::Cached.code()))
        .and_where(Expr::col(JsonldContexts::ExpiresAt).is_not_null());
    scalar(select)
}

/// A scalar subquery of the earliest subscription expiry, ignoring subscriptions
/// already expired.
fn subscription_expiry_min() -> SimpleExpr {
    let mut select = Query::select();
    select
        .expr(Func::min(Expr::col(Subscriptions::ExpiresAt)))
        .from(Subscriptions::Table)
        .and_where(Expr::col(Subscriptions::ExpiresAt).is_not_null())
        .and_where(status_is_not(SubscriptionStatus::Expired));
    scalar(select)
}

/// The active periodic subscriptions' grid anchors and intervals -- entity
/// subscriptions (clause 5.8.6) and context source subscriptions (clause
/// 5.11.7) alike, which share the table -- so the caller can compute the
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

/// An unfinished durable delivery: neither delivered nor exhausted.
fn unfinished_delivery() -> SimpleExpr {
    Expr::col(NotificationDeliveries::DeliveredAt)
        .is_null()
        .and(Expr::col(NotificationDeliveries::ExhaustedAt).is_null())
}

/// An unfinished snapshot status notification: neither delivered nor exhausted.
fn unfinished_snapshot_notification() -> SimpleExpr {
    Expr::col(SnapshotNotificationOutbox::DeliveredAt)
        .is_null()
        .and(Expr::col(SnapshotNotificationOutbox::ExhaustedAt).is_null())
}

/// An unfinished side effect: not yet exhausted.
fn unfinished_side_effect() -> SimpleExpr {
    Expr::col(PendingSideEffects::ExhaustedAt).is_null()
}

/// Wrap a single-row select as a scalar subquery expression.
fn scalar(select: SelectStatement) -> SimpleExpr {
    SimpleExpr::SubQuery(None, Box::new(select.into_sub_query_statement()))
}

#[cfg(test)]
mod tests {
    use crate::{
        migration_set::branch::BranchSchemaVersion,
        pending_work::{PostgresPendingWorkProbe, code_is_undefined_database, earliest_interval_due, interval_probe_select, least_due_select},
    };
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use andromeda_postgresql_codes::jsonld_context_kind::JsonldContextKindCode;
    use andromeda_tenancy::{database_name::TenantDatabaseName, pending_work::PendingWorkError};
    use chrono::{Duration, Utc};
    use sea_query::PostgresQueryBuilder;
    use sqlx::Error as SqlxError;

    #[test]
    fn a_probe_error_is_classified_database_gone_for_3d000() {
        // SQLSTATE 3D000 (invalid_catalog_name) is the deprovisioned-tenant signal.
        assert!(code_is_undefined_database(Some("3D000")));
        assert!(!code_is_undefined_database(Some("42P01")), "an undefined relation is not a gone database");
        assert!(!code_is_undefined_database(None), "an error with no SQLSTATE is not a gone database");

        // A pool-acquire timeout stays saturation; any other non-3D000 fault stays
        // a generic probe failure. (A synthesized 3D000 Database error cannot be
        // built without a driver connection, so the code path above covers it.)
        let database = TenantDatabaseName::from("andromeda_tenant_gone".to_string());
        assert!(matches!(
            PostgresPendingWorkProbe::classify(&SqlxError::PoolTimedOut, &database),
            PendingWorkError::Saturated { .. }
        ));
        assert!(matches!(
            PostgresPendingWorkProbe::classify(&SqlxError::RowNotFound, &database),
            PendingWorkError::ProbeFailed { .. }
        ));
    }

    #[test]
    fn the_probe_reports_an_overdue_interval_as_due_now() {
        let now = Utc::now();
        // A subscription last delivered well over an interval ago is overdue, so
        // its earliest owed notification is in the past and the tenant is woken.
        let overdue = now - Duration::seconds(100);
        let due = earliest_interval_due(vec![(overdue, 5.0)]);
        assert_eq!(due, overdue.checked_add_signed(Duration::seconds(5)));
        assert!(
            due.is_some_and(|instant| instant <= now),
            "an overdue interval subscription is due now, got {due:?}"
        );

        // A subscription whose first grid point is still ahead is due in the future.
        let ahead = earliest_interval_due(vec![(now, 5.0)]);
        assert!(
            ahead.is_some_and(|instant| instant > now),
            "a fresh interval subscription is due a period ahead, got {ahead:?}"
        );

        // A non-positive interval yields no due instant.
        assert_eq!(earliest_interval_due(vec![(now, 0.0)]), None);
    }

    fn least_sql() -> String {
        least_due_select(BranchSchemaVersion::try_from(26_i64).expect("a positive version"), &[ContextTarget::Live]).to_string(PostgresQueryBuilder)
    }

    /// A delivery queued on a published Snapshot branch is an obligation of the
    /// tenant: the drain claims it like a live one, so its retry must wake a
    /// dormant tenant and keep a resident one's drain from resting past it.
    #[test]
    fn the_probe_reads_the_delivery_queue_of_every_published_snapshot_branch() {
        let snapshot = ContextTarget::Snapshot(SnapshotTarget::published(
            SnapshotKey::try_from(7_i64).expect("a positive key"),
            "urn:ngsi-ld:Snapshot:probe".parse().expect("a valid URI"),
        ));
        let sql = least_due_select(
            BranchSchemaVersion::try_from(26_i64).expect("a positive version"),
            &[ContextTarget::Live, snapshot],
        )
        .to_string(PostgresQueryBuilder);
        assert!(sql.contains("FROM \"notification_deliveries\""), "{sql}");
        assert!(sql.contains("FROM \"snap_7\".\"notification_deliveries\""), "{sql}");
    }

    #[test]
    fn the_probe_reads_the_durable_delivery_queue() {
        let sql = least_sql();
        assert!(sql.contains("FROM \"notification_deliveries\""), "{sql}");
        assert!(sql.contains("GREATEST(\"next_attempt_at\", COALESCE(\"lease_until\""), "{sql}");
    }

    #[test]
    fn the_probe_reads_the_snapshot_status_outbox() {
        assert!(least_sql().contains("FROM \"snapshot_notification_outbox\""), "{}", least_sql());
    }

    #[test]
    fn the_probe_reads_the_pending_side_effects_queue() {
        assert!(least_sql().contains("FROM \"pending_side_effects\""), "{}", least_sql());
    }

    #[test]
    fn the_probe_reads_the_temporal_append_intents_queue() {
        assert!(least_sql().contains("FROM \"temporal_append_intents\""), "{}", least_sql());
    }

    #[test]
    fn the_probe_reads_the_preparing_snapshots() {
        let sql = least_sql();
        assert!(sql.contains("FROM \"snapshot_preparation\""), "{sql}");
        assert!(sql.contains("\"status\" = 1"), "only preparing snapshots, by code: {sql}");
        assert!(
            sql.contains("\"snapshot_preparation\".\"branch_schema_version\" >= 26"),
            "a quarantined snapshot never keeps the tenant due: {sql}"
        );
    }

    #[test]
    fn the_probe_reads_entity_and_attribute_expiry() {
        let sql = least_sql();
        assert!(sql.contains("FROM \"entities\""), "{sql}");
        assert!(sql.contains("\"expires_at\""), "{sql}");
        assert!(sql.contains("\"min_attribute_expires_at\""), "{sql}");
    }

    #[test]
    fn the_probe_reads_subscription_expiry() {
        assert!(least_sql().contains("FROM \"subscriptions\""), "{}", least_sql());
    }

    #[test]
    fn the_probe_reads_context_source_registration_expiry() {
        assert!(least_sql().contains("FROM \"context_source_registrations\""), "{}", least_sql());
    }

    #[test]
    fn the_probe_reads_snapshot_and_entity_map_expiry() {
        let sql = least_sql();
        assert!(sql.contains("FROM \"snapshots\""), "{sql}");
        assert!(sql.contains("FROM \"entity_maps\""), "{sql}");
    }

    #[test]
    fn the_probe_reports_a_branch_schema_without_a_control_record_as_due_now() {
        let sql = least_sql();
        assert!(
            sql.contains(r#"(SELECT NOW() WHERE EXISTS(SELECT DISTINCT substring("nspname", '^snap_([1-9][0-9]*)(_stg)?$')::bigint"#),
            "{sql}"
        );
        assert!(sql.contains(r#"NOT EXISTS(SELECT 1 FROM "snapshots" WHERE"#), "{sql}");
    }

    /// Clause 5.13.1 invalidation is due when the earliest `"Cached"` entry
    /// lapses; a hosted entry never lapses.
    #[test]
    fn the_probe_reads_the_earliest_cached_context_expiry() {
        let sql = least_sql();
        assert!(
            sql.contains(&format!(
                "(SELECT MIN(\"expires_at\") FROM \"jsonld_contexts\" WHERE \"kind\" = {} AND \"expires_at\" IS NOT NULL)",
                JsonldContextKindCode::Cached.code()
            )),
            "{sql}"
        );
    }

    /// Entity and context source subscriptions share the table, so one source
    /// serves both kinds of periodic notification.
    #[test]
    fn the_interval_source_reads_every_kind_of_subscription() {
        let interval_sql = interval_probe_select().to_string(PostgresQueryBuilder);
        assert!(!interval_sql.contains("\"kind\""), "no kind filter narrows the periodic source: {interval_sql}");
    }

    #[test]
    fn the_probe_folds_every_source_into_one_least() {
        assert!(least_sql().contains("SELECT LEAST("), "{}", least_sql());
    }

    #[test]
    fn the_probe_ignores_expired_status_subscriptions() {
        // The subscription-expiry source excludes a subscription already
        // expired; the periodic-notification source reads active ones only, so
        // neither an expired nor a paused subscription keeps the tenant due.
        assert!(
            least_sql().contains("\"subscriptions\".\"status\" <> 3"),
            "the subscription-expiry source excludes expired: {}",
            least_sql()
        );
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
