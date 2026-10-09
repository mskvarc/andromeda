//! The durable decision and action state of one maintenance target.

use crate::{
    candidate::{IndexTarget, MaintenanceTarget},
    catalog_counter::CatalogCounter,
    error::MaintenanceError,
    evidence::{
        action_result::ActionResult,
        recorded_decision::RecordedDecision,
        target_code::{stored_target, target_of},
    },
    policy::{DeferralReason, DegradationSample, PolicyDecision, TargetState},
    state::{RegisteredTenant, TenantKey, execute_update},
};
use andromeda_postgresql_codes::maintenance_target::MaintenanceTargetCode;
use andromeda_postgresql_schema::maintenance::PostgresqlMaintenanceTargets;
use chrono::{DateTime, Utc};
use sea_query::{Expr, ExprTrait, Iden, InsertStatement, OnConflict, Order, PostgresQueryBuilder, Query, error::Error as SeaQueryError};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgConnection, Row};

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

/// Read one target's persisted decision state.
pub(crate) async fn load_target_state(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    target: MaintenanceTarget,
) -> Result<TargetState, MaintenanceError> {
    let statement = Query::select()
        .columns([
            PostgresqlMaintenanceTargets::ConsecutiveBreaches,
            PostgresqlMaintenanceTargets::IsActionPending,
            PostgresqlMaintenanceTargets::CooldownUntil,
        ])
        .from(PostgresqlMaintenanceTargets::Table)
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantKey).eq(tenant.key.0))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).eq(stored_target(target).code()))
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_optional(connection).await?;
    row.map_or(Ok(TargetState::default()), |row| {
        let breaches = row.try_get::<i32, _>(PostgresqlMaintenanceTargets::ConsecutiveBreaches.to_string().as_str())?;
        Ok(TargetState {
            consecutive_breaches: u32::try_from(breaches).map_err(|_| MaintenanceError::NegativeCatalogValue {
                counter: CatalogCounter::ConsecutiveBreaches,
                value: i64::from(breaches),
            })?,
            is_action_pending: row.try_get(PostgresqlMaintenanceTargets::IsActionPending.to_string().as_str())?,
            cooldown_until: row.try_get(PostgresqlMaintenanceTargets::CooldownUntil.to_string().as_str())?,
        })
    })
}

/// Return the first pending action, keeping each canary B-tree ahead of its GIN pair.
pub(crate) async fn pending_action(connection: &mut PgConnection, tenant: &RegisteredTenant) -> Result<Option<MaintenanceTarget>, MaintenanceError> {
    let statement = Query::select()
        .column(PostgresqlMaintenanceTargets::Target)
        .from(PostgresqlMaintenanceTargets::Table)
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantKey).eq(tenant.key.0))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::IsActionPending).eq(true))
        .order_by(PostgresqlMaintenanceTargets::Target, Order::Asc)
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let codes = sqlx::query_scalar_with::<_, i16, _>(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(connection)
        .await?;
    let targets = codes
        .into_iter()
        .map(|code| MaintenanceTargetCode::try_from(code).map(target_of))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(first_pending(&targets))
}

/// The pending target to act on first: any target but a GIN queued through a
/// canary, so a canary is rebuilt on an earlier pass than its GIN, and a GIN
/// once nothing else is pending.
fn first_pending(targets: &[MaintenanceTarget]) -> Option<MaintenanceTarget> {
    targets
        .iter()
        .copied()
        .find(|target| match target {
            MaintenanceTarget::Index(index) => index.canary().is_none(),
            MaintenanceTarget::Heap(_) => true,
        })
        .or_else(|| targets.first().copied())
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
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantKey).eq(tenant.key.0))
        .and_where(
            Expr::col(PostgresqlMaintenanceTargets::Target).is_in(IndexTarget::PROBEABLE.map(|target| stored_target(MaintenanceTarget::Index(target)).code())),
        )
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(connection).await?;
    rows.into_iter()
        .map(|row| {
            let code = MaintenanceTargetCode::try_from(row.try_get::<i16, _>(PostgresqlMaintenanceTargets::Target.to_string().as_str())?)?;
            let MaintenanceTarget::Index(target) = target_of(code) else {
                return Err(MaintenanceError::ProbeTargetNotAnIndex(code));
            };
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
        .value(PostgresqlMaintenanceTargets::LastDecision, RecordedDecision::Policy(decision).code().code())
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantKey).eq(tenant.key.0))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).eq(stored_target(target).code()));
    if let Some(sample) = sample {
        statement
            .value(PostgresqlMaintenanceTargets::LastSampledAt, Expr::current_timestamp())
            .value(
                PostgresqlMaintenanceTargets::LastRelationBytes,
                persisted_bytes(sample.relation_bytes, CatalogCounter::SampleRelationBytes)?,
            )
            .value(
                PostgresqlMaintenanceTargets::LastReclaimableBytes,
                persisted_bytes(sample.reclaimable_bytes, CatalogCounter::SampleReclaimableBytes)?,
            )
            .value(PostgresqlMaintenanceTargets::LastDegradedPercent, i16::from(sample.degraded_percent))
            .value(PostgresqlMaintenanceTargets::LastThresholdPercent, i16::from(sample.threshold_percent));
    }
    match decision {
        PolicyDecision::Act => {
            statement.value(PostgresqlMaintenanceTargets::IsActionPending, true);
        }
        PolicyDecision::Queued => {
            statement
                .value(
                    PostgresqlMaintenanceTargets::ConsecutiveBreaches,
                    Expr::col(PostgresqlMaintenanceTargets::ConsecutiveBreaches).add(1),
                )
                .value(PostgresqlMaintenanceTargets::IsActionPending, true);
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
    let paired_gin = match target {
        MaintenanceTarget::Index(index) => index.paired_gin(),
        MaintenanceTarget::Heap(_) => None,
    };
    if decision == PolicyDecision::Queued
        && let Some(gin) = paired_gin
    {
        let gin = MaintenanceTarget::Index(gin);
        ensure_target_state(connection, tenant, gin).await?;
        let paired = Query::update()
            .table(PostgresqlMaintenanceTargets::Table)
            .value(PostgresqlMaintenanceTargets::IsActionPending, true)
            .value(PostgresqlMaintenanceTargets::LastDecision, RecordedDecision::PairedQueued.code().code())
            .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantKey).eq(tenant.key.0))
            .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).eq(stored_target(gin).code()))
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
        .value(PostgresqlMaintenanceTargets::IsActionPending, false)
        .value(PostgresqlMaintenanceTargets::CooldownUntil, cooldown_until)
        .value(PostgresqlMaintenanceTargets::ConsecutiveFailures, 0)
        .value(PostgresqlMaintenanceTargets::LastActionAt, Expr::current_timestamp())
        .value(PostgresqlMaintenanceTargets::LastActionResult, ActionResult::Succeeded.code().code())
        .value(
            PostgresqlMaintenanceTargets::LastBeforeBytes,
            persisted_bytes(before_bytes, CatalogCounter::BeforeBytes)?,
        )
        .value(
            PostgresqlMaintenanceTargets::LastAfterBytes,
            persisted_bytes(after_bytes, CatalogCounter::AfterBytes)?,
        )
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantKey).eq(tenant.key.0))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).eq(stored_target(target).code()))
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
        .value(PostgresqlMaintenanceTargets::LastActionResult, ActionResult::Failed.code().code())
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantKey).eq(tenant.key.0))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).eq(stored_target(target).code()))
        .to_owned();
    execute_update(connection, statement).await
}

/// Insert a target-state row if this is its first sample.
async fn ensure_target_state(connection: &mut PgConnection, tenant: &RegisteredTenant, target: MaintenanceTarget) -> Result<(), MaintenanceError> {
    let (sql, values) = target_state_insert(tenant.key, target)?.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(connection).await?;
    Ok(())
}

/// Build the idempotent target-state insert.
fn target_state_insert(tenant: TenantKey, target: MaintenanceTarget) -> Result<InsertStatement, SeaQueryError> {
    let mut statement = Query::insert();
    statement
        .into_table(PostgresqlMaintenanceTargets::Table)
        .columns([PostgresqlMaintenanceTargets::TenantKey, PostgresqlMaintenanceTargets::Target])
        .values([tenant.0.into(), stored_target(target).code().into()])?
        .on_conflict(
            OnConflict::columns([PostgresqlMaintenanceTargets::TenantKey, PostgresqlMaintenanceTargets::Target])
                .do_nothing()
                .to_owned(),
        );
    Ok(statement.clone())
}

/// Convert a physical byte count to the platform schema's signed storage type.
fn persisted_bytes(value: u64, counter: CatalogCounter) -> Result<i64, MaintenanceError> {
    i64::try_from(value).map_err(|_| MaintenanceError::CatalogValueOverflow { counter, value })
}

#[cfg(test)]
mod tests {
    use crate::{
        candidate::{HeapTarget, IndexTarget, MaintenanceTarget},
        state::target_record::first_pending,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_canary_is_acted_on_before_the_gin_it_queued() {
        for (canary, gin) in [
            (IndexTarget::IdxCaValueString, IndexTarget::IdxCaValueStringTrgm),
            (IndexTarget::UqCurrentAttributesGrain, IndexTarget::IdxCaSubEqKeys),
        ] {
            assert_eq!(
                first_pending(&[MaintenanceTarget::Index(gin), MaintenanceTarget::Index(canary)]),
                Some(MaintenanceTarget::Index(canary))
            );
        }
    }

    #[test]
    fn a_gin_is_acted_on_once_nothing_else_is_pending() {
        assert_eq!(
            first_pending(&[MaintenanceTarget::Index(IndexTarget::IdxCaSubEqKeys)]),
            Some(MaintenanceTarget::Index(IndexTarget::IdxCaSubEqKeys))
        );
        assert_eq!(first_pending(&[]), None);
    }

    #[test]
    fn a_pending_heap_goes_ahead_of_a_gin() {
        assert_eq!(
            first_pending(&[
                MaintenanceTarget::Index(IndexTarget::IdxCaSubEqKeys),
                MaintenanceTarget::Heap(HeapTarget::CurrentAttributes),
            ]),
            Some(MaintenanceTarget::Heap(HeapTarget::CurrentAttributes))
        );
    }
}
