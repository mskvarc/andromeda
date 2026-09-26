//! The platform-database implementation of the tenant wake schedule.
//!
//! Implements the tenancy crate's [`TenantWakeSchedule`](andromeda_tenancy::wake_schedule::TenantWakeSchedule)
//! port over the `tenant_wake_schedule` table. Every operation is a single
//! statement so it composes with a transaction pooler and holds no lock across
//! an await, and the claim is a `FOR UPDATE SKIP LOCKED` reservation so
//! concurrent replicas never claim the same tenant and never block one another.
//! The schedule keys on the `tenants` surrogate `tenant_key`; the port speaks tenant
//! names, so a name is resolved to its key through a scalar subquery on write and
//! the claimed keys are mapped back to names on read.

use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_postgresql_schema::tenancy::{TenantWakeSchedule as WakeScheduleTable, Tenants};
use andromeda_tenancy::wake_schedule::{TenantWakeSchedule, WakeScheduleError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sea_query::{
    Expr,
    ExprTrait,
    InsertStatement,
    LockBehavior,
    LockType,
    NullOrdering,
    OnConflict,
    Order,
    PostgresQueryBuilder,
    Query,
    QueryStatementBuilder,
    SelectStatement,
    SimpleExpr,
    UpdateStatement,
    error::Error as SeaQueryError,
};
use sea_query_sqlx::SqlxBinder;
use sqlx::PgPool;
use std::time::SystemTime;

/// The tenant wake schedule, backed by the platform database.
pub struct PostgresTenantWakeSchedule {
    /// The platform pool the schedule table lives in. Reaches PostgreSQL through
    /// the pooler when one is configured; every statement is single and
    /// self-contained, so transaction pooling is safe.
    main_pool: PgPool,
}

impl PostgresTenantWakeSchedule {
    /// A wake schedule over the platform pool.
    #[must_use]
    pub const fn new(main_pool: PgPool) -> PostgresTenantWakeSchedule {
        PostgresTenantWakeSchedule { main_pool }
    }

    /// Map the keys a claim returned back to their tenant names.
    async fn names_for_keys(&self, keys: Vec<i64>) -> Result<Vec<TenantName>, WakeScheduleError> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let (sql, values) = names_for_keys_query(keys).build_sqlx(PostgresQueryBuilder);
        let names: Vec<String> = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_all(&self.main_pool)
            .await
            .map_err(|error| store_error(&error))?;
        names.into_iter().map(tenant_name).collect()
    }
}

#[async_trait]
impl TenantWakeSchedule for PostgresTenantWakeSchedule {
    async fn record(&self, tenant: &TenantName, probed_at: SystemTime, wake_at: Option<SystemTime>) -> Result<(), WakeScheduleError> {
        let statement = upsert_statement(tenant, probed_at, wake_at).map_err(|error| arity_error(&error))?;
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .execute(&self.main_pool)
            .await
            .map_err(|error| store_error(&error))?;
        Ok(())
    }

    async fn claim_due(&self, now: SystemTime, lease_until: SystemTime, limit: usize) -> Result<Vec<TenantName>, WakeScheduleError> {
        let (sql, values) = claim_statement(now, lease_until, limit).build_sqlx(PostgresQueryBuilder);
        let keys: Vec<i64> = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_all(&self.main_pool)
            .await
            .map_err(|error| store_error(&error))?;
        self.names_for_keys(keys).await
    }

    async fn settle(&self, tenant: &TenantName, probed_at: SystemTime, wake_at: Option<SystemTime>) -> Result<(), WakeScheduleError> {
        // Settling a claimed row is the same upsert as recording: it overwrites
        // the lease `wake_at` the claim installed with the next real due instant
        // (or NULL) and refreshes `probed_at`.
        self.record(tenant, probed_at, wake_at).await
    }

    async fn next_unprobed(&self, limit: usize) -> Result<Vec<TenantName>, WakeScheduleError> {
        let (sql, values) = next_unprobed_query(limit).build_sqlx(PostgresQueryBuilder);
        let names: Vec<String> = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_all(&self.main_pool)
            .await
            .map_err(|error| store_error(&error))?;
        names.into_iter().map(tenant_name).collect()
    }
}

/// A platform-statement failure becomes a wake-schedule store error.
fn store_error(error: &sqlx::Error) -> WakeScheduleError {
    WakeScheduleError::Store { detail: error.to_string() }
}

/// A statement whose column and value arity disagree is an internal invariant,
/// surfaced as a store error since the port exposes no other channel.
fn arity_error(error: &SeaQueryError) -> WakeScheduleError {
    WakeScheduleError::Store { detail: error.to_string() }
}

/// A registry name that is not a valid tenant name is an internal invariant.
fn tenant_name(name: String) -> Result<TenantName, WakeScheduleError> {
    TenantName::try_from(name).map_err(|error| WakeScheduleError::Store {
        detail: format!("the registry holds a name that is not a tenant name: {error}"),
    })
}

/// Upsert `tenant`'s schedule row: probed now, next due at `wake_at` (NULL when
/// nothing is pending), keyed on the tenant's surrogate id.
fn upsert_statement(tenant: &TenantName, probed_at: SystemTime, wake_at: Option<SystemTime>) -> Result<InsertStatement, SeaQueryError> {
    let tenant_key = SimpleExpr::SubQuery(None, Box::new(tenant_key_of(tenant).into_sub_query_statement()));
    let probed = DateTime::<Utc>::from(probed_at);
    let wake = wake_at.map(DateTime::<Utc>::from);
    let mut statement = Query::insert();
    statement
        .into_table(WakeScheduleTable::Table)
        .columns([WakeScheduleTable::TenantKey, WakeScheduleTable::ProbedAt, WakeScheduleTable::WakeAt])
        .values([tenant_key, Expr::val(probed), Expr::val(wake)])?
        .on_conflict(
            OnConflict::column(WakeScheduleTable::TenantKey)
                .update_columns([WakeScheduleTable::ProbedAt, WakeScheduleTable::WakeAt])
                .to_owned(),
        );
    Ok(statement)
}

/// The surrogate id of the tenant called `name`.
fn tenant_key_of(tenant: &TenantName) -> SelectStatement {
    Query::select()
        .column(Tenants::TenantKey)
        .from(Tenants::Table)
        .and_where(Expr::col(Tenants::Name).eq(tenant.as_str()))
        .to_owned()
}

/// Claim up to `limit` tenants due at or before `now`, leasing each to
/// `lease_until` so a crash before the wake settles re-fires it, returning the
/// claimed surrogate keys.
fn claim_statement(now: SystemTime, lease_until: SystemTime, limit: usize) -> UpdateStatement {
    let candidates = Query::select()
        .column(WakeScheduleTable::TenantKey)
        .from(WakeScheduleTable::Table)
        .and_where(Expr::col(WakeScheduleTable::WakeAt).is_not_null())
        .and_where(Expr::col(WakeScheduleTable::WakeAt).lte(DateTime::<Utc>::from(now)))
        .order_by(WakeScheduleTable::WakeAt, Order::Asc)
        .limit(u64::try_from(limit).unwrap_or(u64::MAX))
        .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
        .to_owned();

    Query::update()
        .table(WakeScheduleTable::Table)
        .value(WakeScheduleTable::WakeAt, DateTime::<Utc>::from(lease_until))
        .and_where(Expr::col(WakeScheduleTable::TenantKey).in_subquery(candidates))
        .returning_col(WakeScheduleTable::TenantKey)
        .to_owned()
}

/// The names of the tenants whose surrogate keys are in `keys`, skipping the
/// unnamed default (whose name is NULL).
fn names_for_keys_query(keys: Vec<i64>) -> SelectStatement {
    Query::select()
        .column(Tenants::Name)
        .from(Tenants::Table)
        .and_where(Expr::col(Tenants::TenantKey).is_in(keys))
        .and_where(Expr::col(Tenants::Name).is_not_null())
        .to_owned()
}

/// The named tenants least recently probed, unprobed ones first, up to `limit`.
fn next_unprobed_query(limit: usize) -> SelectStatement {
    Query::select()
        .column((Tenants::Table, Tenants::Name))
        .from(Tenants::Table)
        .left_join(
            WakeScheduleTable::Table,
            Expr::col((WakeScheduleTable::Table, WakeScheduleTable::TenantKey)).equals((Tenants::Table, Tenants::TenantKey)),
        )
        .and_where(Expr::col((Tenants::Table, Tenants::IsUnnamedDefault)).eq(false))
        .and_where(Expr::col((Tenants::Table, Tenants::Name)).is_not_null())
        .order_by_with_nulls((WakeScheduleTable::Table, WakeScheduleTable::ProbedAt), Order::Asc, NullOrdering::First)
        .limit(u64::try_from(limit).unwrap_or(u64::MAX))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::tenant_wake_schedule::{claim_statement, next_unprobed_query, upsert_statement};
    use andromeda_ngsi_ld_lexicon::tenant::TenantName;
    use sea_query::PostgresQueryBuilder;
    use std::time::{Duration, SystemTime};

    fn tenant() -> TenantName {
        TenantName::try_from("acme".to_string()).expect("valid tenant name")
    }

    #[test]
    fn the_upsert_keys_on_the_tenant_key_and_refreshes_on_conflict() {
        let sql = upsert_statement(&tenant(), SystemTime::UNIX_EPOCH, Some(SystemTime::UNIX_EPOCH))
            .expect("arity")
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains("INSERT INTO \"tenant_wake_schedule\""), "{sql}");
        assert!(
            sql.contains("SELECT \"tenant_key\" FROM \"tenants\""),
            "the tenant key is resolved by name: {sql}"
        );
        assert!(sql.contains("ON CONFLICT (\"tenant_key\") DO UPDATE"), "{sql}");
    }

    #[test]
    fn the_claim_skips_locked_rows_and_returns_the_claimed_ids() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let sql = claim_statement(now, now + Duration::from_secs(60), 4).to_string(PostgresQueryBuilder);
        assert!(sql.contains("UPDATE \"tenant_wake_schedule\""), "{sql}");
        assert!(sql.contains("FOR UPDATE SKIP LOCKED"), "{sql}");
        assert!(sql.contains("RETURNING \"tenant_key\""), "{sql}");
        assert!(sql.contains("LIMIT 4"), "{sql}");
    }

    #[test]
    fn the_reconciliation_orders_least_recently_probed_first() {
        let sql = next_unprobed_query(8).to_string(PostgresQueryBuilder);
        assert!(sql.contains("LEFT JOIN \"tenant_wake_schedule\""), "{sql}");
        assert!(sql.contains("\"probed_at\" ASC NULLS FIRST"), "{sql}");
        assert!(sql.contains("LIMIT 8"), "{sql}");
    }
}
