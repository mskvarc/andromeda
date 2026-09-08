//! Platform-database coordination, fair selection and durable evidence.

use crate::{
    candidate::{IndexTarget, MaintenanceTarget},
    error::MaintenanceError,
    policy::{DeferralReason, DegradationSample, PolicyDecision, TargetState},
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_postgresql_schema::{
    maintenance::{PostgresqlMaintenanceTargets, PostgresqlMaintenanceTenants},
    tenancy::Tenants,
};
use andromeda_postgresql_seaquery::advisory_lock::{advisory_unlock, try_advisory_lock};
use andromeda_tenancy::database_name::TenantDatabaseName;
use chrono::{DateTime, Utc};
use sea_query::{
    Expr,
    ExprTrait,
    Iden,
    InsertStatement,
    JoinType,
    NullOrdering,
    OnConflict,
    Order,
    PostgresQueryBuilder,
    Query,
    SelectStatement,
    UpdateStatement,
    error::Error as SeaQueryError,
};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgConnection, Row};
use std::str::FromStr;

/// Stable key for the process-wide lock.
const GLOBAL_LOCK_KEY: &str = "andromeda-postgresql-maintenance";

/// Stable primary key of one platform tenant row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TenantId(i64);

/// One registered database selected independently of the runtime cache.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RegisteredTenant {
    /// Platform row identity used by durable state.
    pub(crate) id: TenantId,
    /// Absent only for the unnamed default tenant.
    pub(crate) name: Option<TenantName>,
    /// PostgreSQL database name.
    pub(crate) database: TenantDatabaseName,
}

/// Durable ordering and cooldown metadata for one detailed index probe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct IndexProbeState {
    /// Allowlisted index represented by the row.
    pub(crate) target: IndexTarget,
    /// Most recent detailed sample, absent before the first probe.
    pub(crate) last_sampled_at: Option<DateTime<Utc>>,
    /// Successful-action cooldown, absent before the first success.
    pub(crate) cooldown_until: Option<DateTime<Utc>>,
}

impl RegisteredTenant {
    /// Bounded log label that never becomes a metric dimension.
    pub(crate) fn log_name(&self) -> &str {
        self.name.as_ref().map_or("<default>", TenantName::as_str)
    }
}

/// Try to obtain the one fleet-wide session advisory lock without waiting.
pub(crate) async fn try_global_lock(connection: &mut PgConnection) -> Result<bool, MaintenanceError> {
    let statement = Query::select().expr(try_advisory_lock(Expr::val(GLOBAL_LOCK_KEY))).to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    Ok(sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(connection).await?)
}

/// Release the fleet-wide session advisory lock.
pub(crate) async fn release_global_lock(connection: &mut PgConnection) -> Result<(), MaintenanceError> {
    let statement = Query::select().expr(advisory_unlock(Expr::val(GLOBAL_LOCK_KEY))).to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let released = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(connection).await?;
    if released {
        Ok(())
    } else {
        Err(MaintenanceError::Database(sqlx::Error::Protocol(
            "the PostgreSQL maintenance advisory lock was not held".to_owned(),
        )))
    }
}

/// Select the registered database with the oldest persisted inspection time.
pub(crate) async fn select_least_recently_inspected(connection: &mut PgConnection) -> Result<Option<RegisteredTenant>, MaintenanceError> {
    let (sql, values) = least_recently_inspected_statement().build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_optional(connection).await?;
    row.map(|row| {
        let raw_name = row.try_get::<Option<String>, _>(Tenants::Name.to_string().as_str())?;
        Ok(RegisteredTenant {
            id: TenantId(row.try_get(Tenants::Id.to_string().as_str())?),
            name: raw_name.map(TenantName::try_from).transpose()?,
            database: TenantDatabaseName::from(row.try_get::<String, _>(Tenants::DbName.to_string().as_str())?),
        })
    })
    .transpose()
}

/// Persist that a tenant received its fair turn even when it was unavailable.
pub(crate) async fn mark_inspected(connection: &mut PgConnection, tenant: &RegisteredTenant, result: &'static str) -> Result<(), MaintenanceError> {
    ensure_tenant_state(connection, tenant).await?;
    let mut statement = Query::update();
    statement
        .table(PostgresqlMaintenanceTenants::Table)
        .value(PostgresqlMaintenanceTenants::LastInspectedAt, Expr::current_timestamp())
        .value(PostgresqlMaintenanceTenants::LastResult, result)
        .and_where(Expr::col(PostgresqlMaintenanceTenants::TenantId).eq(tenant.id.0));
    if result != "started" {
        statement.value(PostgresqlMaintenanceTenants::ConsecutiveFailures, 0);
    }
    execute_update(connection, statement.clone()).await
}

/// Record that reaching one tenant database failed without starving later tenants.
pub(crate) async fn mark_tenant_failure(connection: &mut PgConnection, tenant: &RegisteredTenant) -> Result<(), MaintenanceError> {
    ensure_tenant_state(connection, tenant).await?;
    let statement = Query::update()
        .table(PostgresqlMaintenanceTenants::Table)
        .value(PostgresqlMaintenanceTenants::LastInspectedAt, Expr::current_timestamp())
        .value(
            PostgresqlMaintenanceTenants::ConsecutiveFailures,
            Expr::col(PostgresqlMaintenanceTenants::ConsecutiveFailures).add(1),
        )
        .value(PostgresqlMaintenanceTenants::LastResult, "unavailable")
        .and_where(Expr::col(PostgresqlMaintenanceTenants::TenantId).eq(tenant.id.0))
        .to_owned();
    execute_update(connection, statement).await
}

/// Read one target's persisted decision state.
pub(crate) async fn load_target_state(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    target: MaintenanceTarget,
) -> Result<TargetState, MaintenanceError> {
    let statement = Query::select()
        .columns([
            PostgresqlMaintenanceTargets::ConsecutiveBreaches,
            PostgresqlMaintenanceTargets::PendingAction,
            PostgresqlMaintenanceTargets::CooldownUntil,
        ])
        .from(PostgresqlMaintenanceTargets::Table)
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantId).eq(tenant.id.0))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).eq(target.state_key()))
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_optional(connection).await?;
    row.map_or(Ok(TargetState::default()), |row| {
        let breaches = row.try_get::<i32, _>(PostgresqlMaintenanceTargets::ConsecutiveBreaches.to_string().as_str())?;
        Ok(TargetState {
            consecutive_breaches: u32::try_from(breaches).map_err(|_| MaintenanceError::NegativeCatalogValue {
                field: "consecutive_breaches",
                value: i64::from(breaches),
            })?,
            pending_action: row.try_get(PostgresqlMaintenanceTargets::PendingAction.to_string().as_str())?,
            cooldown_until: row.try_get(PostgresqlMaintenanceTargets::CooldownUntil.to_string().as_str())?,
        })
    })
}

/// Return the first pending action, keeping the string B-tree ahead of its GIN pair.
pub(crate) async fn pending_action(connection: &mut PgConnection, tenant: &RegisteredTenant) -> Result<Option<MaintenanceTarget>, MaintenanceError> {
    let statement = Query::select()
        .column(PostgresqlMaintenanceTargets::Target)
        .from(PostgresqlMaintenanceTargets::Table)
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantId).eq(tenant.id.0))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::PendingAction).eq(true))
        .order_by(PostgresqlMaintenanceTargets::Target, Order::Asc)
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let keys = sqlx::query_scalar_with::<_, String, _>(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(connection)
        .await?;
    let targets = keys
        .into_iter()
        .map(|key| MaintenanceTarget::from_state_key(&key).ok_or(MaintenanceError::UnknownTarget(key)))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(targets
        .iter()
        .copied()
        .find(|target| matches!(target, MaintenanceTarget::Index(IndexTarget::IdxCavAttrString)))
        .or_else(|| {
            targets
                .iter()
                .copied()
                .find(|target| !matches!(target, MaintenanceTarget::Index(IndexTarget::IdxCavAttrStringTrgm)))
        })
        .or_else(|| targets.first().copied()))
}

/// Read detailed-probe ordering and cooldown state in one platform query.
pub(crate) async fn index_probe_states(connection: &mut PgConnection, tenant: &RegisteredTenant) -> Result<Vec<IndexProbeState>, MaintenanceError> {
    let statement = Query::select()
        .columns([
            PostgresqlMaintenanceTargets::Target,
            PostgresqlMaintenanceTargets::LastSampledAt,
            PostgresqlMaintenanceTargets::CooldownUntil,
        ])
        .from(PostgresqlMaintenanceTargets::Table)
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantId).eq(tenant.id.0))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).is_in(IndexTarget::PROBEABLE.map(IndexTarget::relation_name)))
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(connection).await?;
    rows.into_iter()
        .map(|row| {
            let name = row.try_get::<String, _>(PostgresqlMaintenanceTargets::Target.to_string().as_str())?;
            let target = IndexTarget::from_str(&name).map_err(|_| MaintenanceError::UnknownTarget(name))?;
            Ok(IndexProbeState {
                target,
                last_sampled_at: row.try_get(PostgresqlMaintenanceTargets::LastSampledAt.to_string().as_str())?,
                cooldown_until: row.try_get(PostgresqlMaintenanceTargets::CooldownUntil.to_string().as_str())?,
            })
        })
        .collect()
}

/// Persist sample evidence and any newly queued action.
pub(crate) async fn record_decision(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    target: MaintenanceTarget,
    sample: Option<DegradationSample>,
    decision: PolicyDecision,
) -> Result<(), MaintenanceError> {
    ensure_target_state(connection, tenant, target).await?;
    let mut statement = Query::update();
    statement
        .table(PostgresqlMaintenanceTargets::Table)
        .value(PostgresqlMaintenanceTargets::LastDecision, decision_label(decision))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantId).eq(tenant.id.0))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).eq(target.state_key()));
    if let Some(sample) = sample {
        statement
            .value(PostgresqlMaintenanceTargets::LastSampledAt, Expr::current_timestamp())
            .value(
                PostgresqlMaintenanceTargets::LastRelationBytes,
                persisted_bytes(sample.relation_bytes, "sample relation bytes")?,
            )
            .value(
                PostgresqlMaintenanceTargets::LastReclaimableBytes,
                persisted_bytes(sample.reclaimable_bytes, "sample reclaimable bytes")?,
            )
            .value(PostgresqlMaintenanceTargets::LastDegradedPercent, i16::from(sample.degraded_percent))
            .value(PostgresqlMaintenanceTargets::LastThresholdPercent, i16::from(sample.threshold_percent));
    }
    match decision {
        PolicyDecision::Act => {
            statement.value(PostgresqlMaintenanceTargets::PendingAction, true);
        }
        PolicyDecision::Queued => {
            statement
                .value(
                    PostgresqlMaintenanceTargets::ConsecutiveBreaches,
                    Expr::col(PostgresqlMaintenanceTargets::ConsecutiveBreaches).add(1),
                )
                .value(PostgresqlMaintenanceTargets::PendingAction, true);
        }
        PolicyDecision::WouldAct => {
            if sample.is_some() {
                statement.value(
                    PostgresqlMaintenanceTargets::ConsecutiveBreaches,
                    Expr::col(PostgresqlMaintenanceTargets::ConsecutiveBreaches).add(1),
                );
            }
        }
        PolicyDecision::Deferred(DeferralReason::ConsecutiveSamples) => {
            statement.value(
                PostgresqlMaintenanceTargets::ConsecutiveBreaches,
                Expr::col(PostgresqlMaintenanceTargets::ConsecutiveBreaches).add(1),
            );
        }
        PolicyDecision::Deferred(DeferralReason::RelationTooSmall | DeferralReason::ReclaimTooSmall | DeferralReason::BelowThreshold) => {
            statement.value(PostgresqlMaintenanceTargets::ConsecutiveBreaches, 0);
        }
        PolicyDecision::Deferred(
            DeferralReason::Cooldown
            | DeferralReason::ActiveBackendRatio
            | DeferralReason::LongTransaction
            | DeferralReason::ActiveMaintenance
            | DeferralReason::WaitingRelationLock,
        ) => {}
    }
    execute_update(connection, statement.clone()).await?;
    if decision == PolicyDecision::Queued && matches!(target, MaintenanceTarget::Index(IndexTarget::IdxCavAttrString)) {
        let trigram = MaintenanceTarget::Index(IndexTarget::IdxCavAttrStringTrgm);
        ensure_target_state(connection, tenant, trigram).await?;
        let paired = Query::update()
            .table(PostgresqlMaintenanceTargets::Table)
            .value(PostgresqlMaintenanceTargets::PendingAction, true)
            .value(PostgresqlMaintenanceTargets::LastDecision, "paired-queued")
            .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantId).eq(tenant.id.0))
            .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).eq(trigram.state_key()))
            .to_owned();
        execute_update(connection, paired).await?;
    }
    Ok(())
}

/// Persist a successful action, its cooldown and measured byte delta.
pub(crate) async fn record_action_success(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    target: MaintenanceTarget,
    cooldown_until: DateTime<Utc>,
    before_bytes: u64,
    after_bytes: u64,
) -> Result<(), MaintenanceError> {
    let statement = Query::update()
        .table(PostgresqlMaintenanceTargets::Table)
        .value(PostgresqlMaintenanceTargets::ConsecutiveBreaches, 0)
        .value(PostgresqlMaintenanceTargets::PendingAction, false)
        .value(PostgresqlMaintenanceTargets::CooldownUntil, cooldown_until)
        .value(PostgresqlMaintenanceTargets::ConsecutiveFailures, 0)
        .value(PostgresqlMaintenanceTargets::LastActionAt, Expr::current_timestamp())
        .value(PostgresqlMaintenanceTargets::LastActionResult, "succeeded")
        .value(PostgresqlMaintenanceTargets::LastBeforeBytes, persisted_bytes(before_bytes, "before bytes")?)
        .value(PostgresqlMaintenanceTargets::LastAfterBytes, persisted_bytes(after_bytes, "after bytes")?)
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantId).eq(tenant.id.0))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).eq(target.state_key()))
        .to_owned();
    execute_update(connection, statement).await
}

/// Persist a failed action while keeping it pending for a later retry.
pub(crate) async fn record_action_failure(connection: &mut PgConnection, tenant: &RegisteredTenant, target: MaintenanceTarget) -> Result<(), MaintenanceError> {
    let statement = Query::update()
        .table(PostgresqlMaintenanceTargets::Table)
        .value(
            PostgresqlMaintenanceTargets::ConsecutiveFailures,
            Expr::col(PostgresqlMaintenanceTargets::ConsecutiveFailures).add(1),
        )
        .value(PostgresqlMaintenanceTargets::LastActionAt, Expr::current_timestamp())
        .value(PostgresqlMaintenanceTargets::LastActionResult, "failed")
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantId).eq(tenant.id.0))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).eq(target.state_key()))
        .to_owned();
    execute_update(connection, statement).await
}

/// Fair-selection query over every active registry row, including the default.
fn least_recently_inspected_statement() -> SelectStatement {
    Query::select()
        .columns([
            (Tenants::Table, Tenants::Id),
            (Tenants::Table, Tenants::Name),
            (Tenants::Table, Tenants::DbName),
        ])
        .from(Tenants::Table)
        .join(
            JoinType::LeftJoin,
            PostgresqlMaintenanceTenants::Table,
            Expr::col((PostgresqlMaintenanceTenants::Table, PostgresqlMaintenanceTenants::TenantId)).equals((Tenants::Table, Tenants::Id)),
        )
        .and_where(Expr::col((Tenants::Table, Tenants::Status)).eq("active"))
        .order_by_with_nulls(
            (PostgresqlMaintenanceTenants::Table, PostgresqlMaintenanceTenants::LastInspectedAt),
            Order::Asc,
            NullOrdering::First,
        )
        .order_by((Tenants::Table, Tenants::Id), Order::Asc)
        .limit(1)
        .to_owned()
}

/// Insert a tenant-state row if this is its first maintenance pass.
async fn ensure_tenant_state(connection: &mut PgConnection, tenant: &RegisteredTenant) -> Result<(), MaintenanceError> {
    let (sql, values) = tenant_state_insert(tenant.id)?.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(connection).await?;
    Ok(())
}

/// Insert a target-state row if this is its first sample.
async fn ensure_target_state(connection: &mut PgConnection, tenant: &RegisteredTenant, target: MaintenanceTarget) -> Result<(), MaintenanceError> {
    let (sql, values) = target_state_insert(tenant.id, target)?.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(connection).await?;
    Ok(())
}

/// Build the idempotent tenant-state insert.
fn tenant_state_insert(tenant: TenantId) -> Result<InsertStatement, SeaQueryError> {
    let mut statement = Query::insert();
    statement
        .into_table(PostgresqlMaintenanceTenants::Table)
        .columns([PostgresqlMaintenanceTenants::TenantId])
        .values([tenant.0.into()])?
        .on_conflict(OnConflict::column(PostgresqlMaintenanceTenants::TenantId).do_nothing().to_owned());
    Ok(statement.clone())
}

/// Build the idempotent target-state insert.
fn target_state_insert(tenant: TenantId, target: MaintenanceTarget) -> Result<InsertStatement, SeaQueryError> {
    let mut statement = Query::insert();
    statement
        .into_table(PostgresqlMaintenanceTargets::Table)
        .columns([PostgresqlMaintenanceTargets::TenantId, PostgresqlMaintenanceTargets::Target])
        .values([tenant.0.into(), target.state_key().into()])?
        .on_conflict(
            OnConflict::columns([PostgresqlMaintenanceTargets::TenantId, PostgresqlMaintenanceTargets::Target])
                .do_nothing()
                .to_owned(),
        );
    Ok(statement.clone())
}

/// Execute one SeaQuery-built update.
async fn execute_update(connection: &mut PgConnection, statement: UpdateStatement) -> Result<(), MaintenanceError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(connection).await?;
    Ok(())
}

/// Convert a physical byte count to the platform schema's signed storage type.
fn persisted_bytes(value: u64, field: &'static str) -> Result<i64, MaintenanceError> {
    i64::try_from(value).map_err(|_| MaintenanceError::CatalogValueOverflow { field, value })
}

/// Stable bounded policy label persisted for operator inspection.
const fn decision_label(decision: PolicyDecision) -> &'static str {
    match decision {
        PolicyDecision::Act => "act",
        PolicyDecision::Queued => "queued",
        PolicyDecision::WouldAct => "would-act",
        PolicyDecision::Deferred(DeferralReason::RelationTooSmall) => "relation-too-small",
        PolicyDecision::Deferred(DeferralReason::ReclaimTooSmall) => "reclaim-too-small",
        PolicyDecision::Deferred(DeferralReason::BelowThreshold) => "below-threshold",
        PolicyDecision::Deferred(DeferralReason::ConsecutiveSamples) => "consecutive-samples",
        PolicyDecision::Deferred(DeferralReason::Cooldown) => "cooldown",
        PolicyDecision::Deferred(DeferralReason::ActiveBackendRatio) => "active-backend-ratio",
        PolicyDecision::Deferred(DeferralReason::LongTransaction) => "long-transaction",
        PolicyDecision::Deferred(DeferralReason::ActiveMaintenance) => "active-maintenance",
        PolicyDecision::Deferred(DeferralReason::WaitingRelationLock) => "waiting-relation-lock",
    }
}

#[cfg(test)]
mod tests {
    use crate::state::{RegisteredTenant, TenantId, least_recently_inspected_statement};
    use andromeda_tenancy::database_name::TenantDatabaseName;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn fair_selection_includes_default_and_orders_never_inspected_first() {
        let sql = least_recently_inspected_statement().to_string(PostgresQueryBuilder);
        assert!(sql.contains("LEFT JOIN \"postgresql_maintenance_tenants\""), "{sql}");
        assert!(sql.contains("NULLS FIRST"), "{sql}");
        assert!(!sql.contains("is_unnamed_default"), "the default must not be filtered: {sql}");
    }

    #[test]
    fn unnamed_default_has_an_explicit_log_label() {
        let tenant = RegisteredTenant {
            id: TenantId(1),
            name: None,
            database: TenantDatabaseName::from("postgres".to_owned()),
        };
        assert_eq!(tenant.log_name(), "<default>");
    }
}
