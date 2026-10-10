//! The durable decision and action state of one maintenance target.

use crate::{
    action_measurement::ActionComparison,
    candidate::{IndexTarget, MaintenanceTarget},
    catalog_counter::{CatalogCounter, persisted_count},
    error::MaintenanceError,
    evidence::{
        action_result::ActionResult,
        recorded_decision::RecordedDecision,
        target_code::{stored_target, target_of},
    },
    policy::{ActionVerdict, BreachRun, DegradationSample, PolicyDecision, TargetState, breach_after},
    state::{RegisteredTenant, TenantKey, execute_update},
};
use andromeda_postgresql_codes::maintenance_target::MaintenanceTargetCode;
use andromeda_postgresql_schema::maintenance::PostgresqlMaintenanceTargets;
use chrono::{DateTime, Utc};
use sea_query::{
    Expr,
    ExprTrait,
    Iden,
    InsertStatement,
    OnConflict,
    Order,
    PostgresQueryBuilder,
    Query,
    SelectStatement,
    UpdateStatement,
    error::Error as SeaQueryError,
};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgConnection, Row, postgres::PgRow};

/// Durable ordering and cooldown metadata for one detailed index probe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct IndexProbeState {
    /// Allowlisted index represented by the row.
    pub(crate) target: IndexTarget,
    /// Most recent detailed sample, absent before the first probe.
    pub(crate) last_sampled_at: Option<DateTime<Utc>>,
    /// Cooldown its last action set, absent before the first action.
    pub(crate) cooldown_until: Option<DateTime<Utc>>,
}

/// One queued action and the cooldown its last attempt set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingAction {
    /// The queued target.
    target: MaintenanceTarget,
    /// Backoff deadline of a failed attempt, absent before any attempt.
    cooldown_until: Option<DateTime<Utc>>,
}

/// Read one target's persisted decision state.
pub(crate) async fn load_target_state(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    target: MaintenanceTarget,
) -> Result<TargetState, MaintenanceError> {
    let statement = Query::select()
        .columns([
            PostgresqlMaintenanceTargets::FirstBreachAt,
            PostgresqlMaintenanceTargets::BreachSamples,
            PostgresqlMaintenanceTargets::IsActionPending,
            PostgresqlMaintenanceTargets::CooldownUntil,
            PostgresqlMaintenanceTargets::ConsecutiveFailures,
        ])
        .from(PostgresqlMaintenanceTargets::Table)
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantKey).eq(tenant.key.0))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).eq(stored_target(target).code()))
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_optional(connection).await?;
    row.map_or(Ok(TargetState::default()), |row| {
        let first_breach_at: Option<DateTime<Utc>> = row.try_get(PostgresqlMaintenanceTargets::FirstBreachAt.to_string().as_str())?;
        let breach_samples = stored_count(&row, PostgresqlMaintenanceTargets::BreachSamples, CatalogCounter::BreachSamples)?;
        Ok(TargetState {
            breach: first_breach_at.map(|since| BreachRun {
                since,
                samples: breach_samples,
            }),
            is_action_pending: row.try_get(PostgresqlMaintenanceTargets::IsActionPending.to_string().as_str())?,
            cooldown_until: row.try_get(PostgresqlMaintenanceTargets::CooldownUntil.to_string().as_str())?,
            consecutive_failures: stored_count(&row, PostgresqlMaintenanceTargets::ConsecutiveFailures, CatalogCounter::ConsecutiveFailures)?,
        })
    })
}

/// Convert a breach run's sample count to the platform schema's `integer`,
/// zero when no run is open.
fn persisted_breach_samples(breach: Option<BreachRun>) -> Result<i32, MaintenanceError> {
    let samples = breach.map_or(0, |run| run.samples);
    i32::try_from(samples).map_err(|_| MaintenanceError::CatalogValueOverflow {
        counter: CatalogCounter::BreachSamples,
        value: u64::from(samples),
    })
}

/// Read one non-negative `integer` counter of a target row.
fn stored_count(row: &PgRow, column: PostgresqlMaintenanceTargets, counter: CatalogCounter) -> Result<u32, MaintenanceError> {
    let value = row.try_get::<i32, _>(column.to_string().as_str())?;
    u32::try_from(value).map_err(|_| MaintenanceError::NegativeCatalogValue {
        counter,
        value: i64::from(value),
    })
}

/// Return the first pending action whose backoff has elapsed, keeping the
/// canary B-tree ahead of its GIN pair.
pub(crate) async fn pending_action(connection: &mut PgConnection, tenant: &RegisteredTenant) -> Result<Option<MaintenanceTarget>, MaintenanceError> {
    let statement = pending_actions_statement()
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantKey).eq(tenant.key.0))
        .to_owned();
    let pending = read_pending_actions(connection, &statement).await?;
    let pending = pending.into_iter().map(|(_, action)| action).collect::<Vec<_>>();
    Ok(first_pending(&pending, Utc::now()))
}

/// Every tenant that [`pending_action`] would give an action to now.
pub(crate) async fn tenants_with_due_actions(connection: &mut PgConnection, now: DateTime<Utc>) -> Result<Vec<TenantKey>, MaintenanceError> {
    let pending = read_pending_actions(connection, &pending_actions_statement()).await?;
    Ok(pending
        .chunk_by(|(left, _), (right, _)| left == right)
        .filter_map(|tenant_pending| {
            let actions = tenant_pending.iter().map(|&(_, action)| action).collect::<Vec<_>>();
            first_pending(&actions, now).and(tenant_pending.first().map(|&(tenant, _)| tenant))
        })
        .collect())
}

/// Every pending action, grouped by tenant and ordered by target code.
fn pending_actions_statement() -> SelectStatement {
    Query::select()
        .columns([
            PostgresqlMaintenanceTargets::TenantKey,
            PostgresqlMaintenanceTargets::Target,
            PostgresqlMaintenanceTargets::CooldownUntil,
        ])
        .from(PostgresqlMaintenanceTargets::Table)
        .and_where(Expr::col(PostgresqlMaintenanceTargets::IsActionPending).eq(true))
        .order_by(PostgresqlMaintenanceTargets::TenantKey, Order::Asc)
        .order_by(PostgresqlMaintenanceTargets::Target, Order::Asc)
        .to_owned()
}

/// Run a pending-action statement and decode each row with its tenant.
async fn read_pending_actions(connection: &mut PgConnection, statement: &SelectStatement) -> Result<Vec<(TenantKey, PendingAction)>, MaintenanceError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(connection).await?;
    rows.iter()
        .map(|row| {
            let tenant = TenantKey(row.try_get(PostgresqlMaintenanceTargets::TenantKey.to_string().as_str())?);
            let action = PendingAction {
                target: target_of(MaintenanceTargetCode::try_from(
                    row.try_get::<i16, _>(PostgresqlMaintenanceTargets::Target.to_string().as_str())?,
                )?),
                cooldown_until: row.try_get(PostgresqlMaintenanceTargets::CooldownUntil.to_string().as_str())?,
            };
            Ok((tenant, action))
        })
        .collect()
}

/// The pending target to act on first: any due target but a GIN queued
/// through a canary, so a canary is rebuilt on an earlier pass than its GIN,
/// and a due canary-queued GIN once nothing else is pending. A target still in its failure
/// backoff is skipped, so it does not hold back the rest of the tenant's pass,
/// yet it still holds back every canary-queued GIN.
fn first_pending(pending: &[PendingAction], now: DateTime<Utc>) -> Option<MaintenanceTarget> {
    let is_gin = |action: &PendingAction| match action.target {
        MaintenanceTarget::Index(index) => index.canary().is_some(),
        MaintenanceTarget::Heap(_) => false,
    };
    let is_due = |action: &PendingAction| action.cooldown_until.is_none_or(|until| until <= now);
    if pending.iter().all(is_gin) {
        pending.iter().find(|action| is_due(action))
    } else {
        pending.iter().find(|action| !is_gin(action) && is_due(action))
    }
    .map(|action| action.target)
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

/// Persist sample evidence, the breach run `decision` leaves of `prior` (the
/// run read before deciding) and any newly queued action. `now` is the instant
/// the decision was taken at.
pub(crate) async fn record_decision(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    target: MaintenanceTarget,
    sample: Option<DegradationSample>,
    decision: PolicyDecision,
    prior: Option<BreachRun>,
    now: DateTime<Utc>,
) -> Result<(), MaintenanceError> {
    ensure_target_state(connection, tenant, target).await?;
    let breach = breach_after(prior, decision, sample, now);
    let mut statement = Query::update();
    statement
        .table(PostgresqlMaintenanceTargets::Table)
        .value(PostgresqlMaintenanceTargets::LastDecision, RecordedDecision::Policy(decision).code().code())
        .value(PostgresqlMaintenanceTargets::FirstBreachAt, breach.map(|run| run.since))
        .value(PostgresqlMaintenanceTargets::BreachSamples, persisted_breach_samples(breach)?)
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantKey).eq(tenant.key.0))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).eq(stored_target(target).code()));
    if let Some(sample) = sample {
        statement
            .value(PostgresqlMaintenanceTargets::LastSampledAt, Expr::current_timestamp())
            .value(
                PostgresqlMaintenanceTargets::LastRelationBytes,
                persisted_count(sample.relation_bytes, CatalogCounter::SampleRelationBytes)?,
            )
            .value(
                PostgresqlMaintenanceTargets::LastReclaimableBytes,
                persisted_count(sample.reclaimable_bytes, CatalogCounter::SampleReclaimableBytes)?,
            )
            .value(PostgresqlMaintenanceTargets::LastDegradedPercent, i16::from(sample.degraded_percent))
            .value(PostgresqlMaintenanceTargets::LastThresholdPercent, i16::from(sample.threshold_percent));
    }
    if matches!(decision, PolicyDecision::Act | PolicyDecision::Queued) {
        statement.value(PostgresqlMaintenanceTargets::IsActionPending, true);
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

/// Persist a completed action, its verdict, its cooldown and the readings
/// around it. Either verdict leaves nothing pending: an ineffective vacuum is
/// queued again only by fresh samples once its shorter cooldown ends.
pub(crate) async fn record_action_completion(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    target: MaintenanceTarget,
    verdict: ActionVerdict,
    cooldown_until: DateTime<Utc>,
    comparison: ActionComparison,
) -> Result<(), MaintenanceError> {
    let statement = action_completion_update(tenant.key, target, verdict, cooldown_until, comparison)?;
    execute_update(connection, statement).await
}

/// Build the completed-action update, recording the readings of the kind the
/// action is measured by and clearing the other kind.
fn action_completion_update(
    tenant: TenantKey,
    target: MaintenanceTarget,
    verdict: ActionVerdict,
    cooldown_until: DateTime<Utc>,
    comparison: ActionComparison,
) -> Result<UpdateStatement, MaintenanceError> {
    let (before_bytes, after_bytes, dead_tuples_before, dead_tuples_after) = match comparison {
        ActionComparison::Index { before_bytes, after_bytes } => (
            Some(persisted_count(before_bytes, CatalogCounter::BeforeBytes)?),
            Some(persisted_count(after_bytes, CatalogCounter::AfterBytes)?),
            None,
            None,
        ),
        ActionComparison::Heap { before, after } => (
            None,
            None,
            Some(persisted_count(before.dead, CatalogCounter::BeforeDeadTuples)?),
            Some(persisted_count(after.dead, CatalogCounter::AfterDeadTuples)?),
        ),
    };
    Ok(Query::update()
        .table(PostgresqlMaintenanceTargets::Table)
        .value(PostgresqlMaintenanceTargets::FirstBreachAt, Option::<DateTime<Utc>>::None)
        .value(PostgresqlMaintenanceTargets::BreachSamples, 0)
        .value(PostgresqlMaintenanceTargets::IsActionPending, false)
        .value(PostgresqlMaintenanceTargets::CooldownUntil, cooldown_until)
        .value(PostgresqlMaintenanceTargets::ConsecutiveFailures, 0)
        .value(PostgresqlMaintenanceTargets::LastActionAt, Expr::current_timestamp())
        .value(PostgresqlMaintenanceTargets::LastActionResult, ActionResult::of_verdict(verdict).code().code())
        .value(PostgresqlMaintenanceTargets::LastBeforeBytes, before_bytes)
        .value(PostgresqlMaintenanceTargets::LastAfterBytes, after_bytes)
        .value(PostgresqlMaintenanceTargets::LastDeadTuplesBefore, dead_tuples_before)
        .value(PostgresqlMaintenanceTargets::LastDeadTuplesAfter, dead_tuples_after)
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantKey).eq(tenant.0))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).eq(stored_target(target).code()))
        .to_owned())
}

/// Persist a failed action while keeping it pending for a retry once its
/// backoff ends.
pub(crate) async fn record_action_failure(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    target: MaintenanceTarget,
    cooldown_until: DateTime<Utc>,
) -> Result<(), MaintenanceError> {
    let statement = Query::update()
        .table(PostgresqlMaintenanceTargets::Table)
        .value(
            PostgresqlMaintenanceTargets::ConsecutiveFailures,
            Expr::col(PostgresqlMaintenanceTargets::ConsecutiveFailures).add(1),
        )
        .value(PostgresqlMaintenanceTargets::CooldownUntil, cooldown_until)
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

#[cfg(test)]
mod tests {
    use crate::{
        action_measurement::{ActionComparison, TupleCounts},
        candidate::{HeapTarget, IndexTarget, MaintenanceTarget},
        error::MaintenanceError,
        policy::ActionVerdict,
        state::{
            TenantKey,
            target_record::{PendingAction, action_completion_update, first_pending},
        },
    };
    use chrono::{DateTime, Duration, Utc};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn a_vacuum_records_dead_tuples_and_clears_the_byte_pair() -> Result<(), MaintenanceError> {
        let comparison = ActionComparison::Heap {
            before: TupleCounts { dead: 50_000, live: 100_000 },
            after: TupleCounts { dead: 1_000, live: 100_000 },
        };
        let sql = action_completion_update(
            TenantKey(7),
            MaintenanceTarget::Heap(HeapTarget::CurrentAttributes),
            ActionVerdict::Effective,
            Utc::now(),
            comparison,
        )?
        .to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""last_action_result" = 1"#), "{sql}");
        assert!(sql.contains(r#""last_dead_tuples_before" = 50000"#), "{sql}");
        assert!(sql.contains(r#""last_dead_tuples_after" = 1000"#), "{sql}");
        assert!(sql.contains(r#""last_before_bytes" = NULL"#), "{sql}");
        assert!(sql.contains(r#""last_after_bytes" = NULL"#), "{sql}");
        Ok(())
    }

    #[test]
    fn a_rebuild_records_bytes_and_clears_the_dead_tuple_pair() -> Result<(), MaintenanceError> {
        let comparison = ActionComparison::Index {
            before_bytes: 104_857_600,
            after_bytes: 41_943_040,
        };
        let sql = action_completion_update(
            TenantKey(7),
            MaintenanceTarget::Index(IndexTarget::IdxCaValueNumber),
            ActionVerdict::Effective,
            Utc::now(),
            comparison,
        )?
        .to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""last_before_bytes" = 104857600"#), "{sql}");
        assert!(sql.contains(r#""last_after_bytes" = 41943040"#), "{sql}");
        assert!(sql.contains(r#""last_dead_tuples_before" = NULL"#), "{sql}");
        assert!(sql.contains(r#""last_dead_tuples_after" = NULL"#), "{sql}");
        Ok(())
    }

    #[test]
    fn an_ineffective_vacuum_records_its_result_and_leaves_nothing_pending() -> Result<(), MaintenanceError> {
        let comparison = ActionComparison::Heap {
            before: TupleCounts { dead: 50_000, live: 80_000 },
            after: TupleCounts { dead: 20_000, live: 80_000 },
        };
        let sql = action_completion_update(
            TenantKey(7),
            MaintenanceTarget::Heap(HeapTarget::CurrentAttributes),
            ActionVerdict::Ineffective,
            Utc::now(),
            comparison,
        )?
        .to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""last_action_result" = 3"#), "{sql}");
        assert!(sql.contains(r#""is_action_pending" = FALSE"#), "{sql}");
        assert!(sql.contains(r#""consecutive_failures" = 0"#), "{sql}");
        assert!(sql.contains(r#""first_breach_at" = NULL"#), "{sql}");
        assert!(sql.contains(r#""breach_samples" = 0"#), "{sql}");
        Ok(())
    }

    /// A pending target never attempted, so due at once.
    const fn due(target: MaintenanceTarget) -> PendingAction {
        PendingAction { target, cooldown_until: None }
    }

    /// A pending target whose failure backoff ends after `now`.
    fn backing_off(target: MaintenanceTarget, now: DateTime<Utc>) -> PendingAction {
        PendingAction {
            target,
            cooldown_until: Some(now + Duration::minutes(15)),
        }
    }

    #[test]
    fn a_canary_is_acted_on_before_the_gin_it_queued() {
        let canary = MaintenanceTarget::Index(IndexTarget::IdxCaValueString);
        let gin = MaintenanceTarget::Index(IndexTarget::IdxCaValueStringTrgm);
        assert_eq!(first_pending(&[due(gin), due(canary)], Utc::now()), Some(canary));
    }

    #[test]
    fn a_gin_is_acted_on_once_nothing_else_is_pending() {
        assert_eq!(
            first_pending(&[due(MaintenanceTarget::Index(IndexTarget::IdxCaValueStringTrgm))], Utc::now()),
            Some(MaintenanceTarget::Index(IndexTarget::IdxCaValueStringTrgm))
        );
        assert_eq!(first_pending(&[], Utc::now()), None);
    }

    #[test]
    fn a_pending_heap_goes_ahead_of_a_gin() {
        assert_eq!(
            first_pending(
                &[
                    due(MaintenanceTarget::Index(IndexTarget::IdxCaValueStringTrgm)),
                    due(MaintenanceTarget::Heap(HeapTarget::CurrentAttributes)),
                ],
                Utc::now()
            ),
            Some(MaintenanceTarget::Heap(HeapTarget::CurrentAttributes))
        );
    }

    /// A density-measured GIN is queued by its own sample, so no canary has
    /// to be rebuilt first: it is acted on in target order like a B-tree.
    #[test]
    fn a_density_measured_gin_is_not_held_back_behind_other_work() {
        let gin = MaintenanceTarget::Index(IndexTarget::IdxCaSubEqKeys);
        let heap = MaintenanceTarget::Heap(HeapTarget::Entities);
        assert_eq!(first_pending(&[due(gin), due(heap)], Utc::now()), Some(gin));
    }

    #[test]
    fn a_backing_off_action_is_skipped_and_still_holds_back_its_gin() {
        let now = Utc::now();
        let canary = MaintenanceTarget::Index(IndexTarget::IdxCaValueString);
        let gin = MaintenanceTarget::Index(IndexTarget::IdxCaValueStringTrgm);
        let heap = MaintenanceTarget::Heap(HeapTarget::Entities);
        assert_eq!(first_pending(&[backing_off(canary, now), due(heap)], now), Some(heap));
        assert_eq!(first_pending(&[backing_off(canary, now), due(gin)], now), None);
        assert_eq!(first_pending(&[backing_off(gin, now)], now), None);
        assert_eq!(first_pending(&[backing_off(canary, now)], now + Duration::minutes(15)), Some(canary));
    }
}
