//! The durable decision and action state of one maintenance target.

use crate::{
    action_measurement::ActionComparison,
    action_policy::ActionVerdict,
    branch::{Branch, BranchTarget},
    candidate::{MaintenanceTarget, index_target::IndexTarget},
    catalog_counter::{CatalogCounter, persisted_count},
    error::MaintenanceError,
    evidence::{
        action_result::ActionResult,
        recorded_decision::RecordedDecision,
        target_code::{stored_target, target_of},
    },
    policy::{BreachRun, DeferralReason, DegradationSample, PolicyDecision, TargetState, breach_after},
    state::{
        RegisteredTenant,
        TenantKey,
        branch_rows::{branch_rows, stored_snapshot_key},
    },
};
use andromeda_postgresql_codes::maintenance_target::MaintenanceTargetCode;
use andromeda_postgresql_schema::maintenance::PostgresqlMaintenanceTargets;
use andromeda_postgresql_seaquery::statement::Statement;
use chrono::{DateTime, Utc};
use sea_query::{Expr, ExprTrait, Iden, InsertStatement, OnConflict, Query, UpdateStatement, error::Error as SeaQueryError};
use sqlx::{PgConnection, Row, postgres::PgRow};

/// The columns identifying one target row: its tenant, its branch and its
/// target.
pub(crate) const TARGET_GRAIN: [PostgresqlMaintenanceTargets; 3] = [
    PostgresqlMaintenanceTargets::TenantKey,
    PostgresqlMaintenanceTargets::SnapshotKey,
    PostgresqlMaintenanceTargets::Target,
];

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

/// Read one target's persisted decision state.
pub(crate) async fn load_target_state(connection: &mut PgConnection, tenant: &RegisteredTenant, target: BranchTarget) -> Result<TargetState, MaintenanceError> {
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
        .and_where(branch_rows(target.branch()))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).eq(stored_target(target.target()).code()))
        .to_owned();
    let row = statement.to_query().fetch_optional(connection).await?;
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

/// Read the detailed-probe ordering and cooldown state of `branch` in one
/// platform query.
pub(crate) async fn index_probe_states(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    branch: Branch,
) -> Result<Vec<IndexProbeState>, MaintenanceError> {
    let statement = Query::select()
        .columns([
            PostgresqlMaintenanceTargets::Target,
            PostgresqlMaintenanceTargets::LastSampledAt,
            PostgresqlMaintenanceTargets::CooldownUntil,
        ])
        .from(PostgresqlMaintenanceTargets::Table)
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantKey).eq(tenant.key.0))
        .and_where(branch_rows(branch))
        .and_where(
            Expr::col(PostgresqlMaintenanceTargets::Target).is_in(IndexTarget::PROBEABLE.map(|target| stored_target(MaintenanceTarget::Index(target)).code())),
        )
        .to_owned();
    let rows = statement.to_query().fetch_all(connection).await?;
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
    target: BranchTarget,
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
        .and_where(branch_rows(target.branch()))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).eq(stored_target(target.target()).code()));
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
    statement.to_query().execute(&mut *connection).await?;
    let paired_gin = match target.target() {
        MaintenanceTarget::Index(index) => index.paired_gin(),
        MaintenanceTarget::Heap(_) | MaintenanceTarget::Statistics(_) | MaintenanceTarget::Columnstore(_) | MaintenanceTarget::Rowstore(_) => None,
    };
    if decision == PolicyDecision::Queued
        && let Some(gin) = paired_gin
    {
        let gin = BranchTarget::new(target.branch(), MaintenanceTarget::Index(gin))?;
        ensure_target_state(connection, tenant, gin).await?;
        let paired = Query::update()
            .table(PostgresqlMaintenanceTargets::Table)
            .value(PostgresqlMaintenanceTargets::IsActionPending, true)
            .value(PostgresqlMaintenanceTargets::LastDecision, RecordedDecision::PairedQueued.code().code())
            .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantKey).eq(tenant.key.0))
            .and_where(branch_rows(gin.branch()))
            .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).eq(stored_target(gin.target()).code()))
            .to_owned();
        paired.to_query().execute(&mut *connection).await?;
    }
    Ok(())
}

/// Persist a completed action, its verdict, its cooldown and the readings
/// around it. Either verdict leaves nothing pending: an ineffective vacuum is
/// queued again only by fresh samples once its shorter cooldown ends.
pub(crate) async fn record_action_completion(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    target: BranchTarget,
    verdict: ActionVerdict,
    cooldown_until: DateTime<Utc>,
    comparison: ActionComparison,
) -> Result<(), MaintenanceError> {
    action_completion_update(tenant.key, target, verdict, cooldown_until, comparison)?
        .to_query()
        .execute(connection)
        .await?;
    Ok(())
}

/// Build the completed-action update, recording the readings of the kind the
/// action is measured by and clearing the other kind. An analyze and a
/// recompression record neither.
fn action_completion_update(
    tenant: TenantKey,
    target: BranchTarget,
    verdict: ActionVerdict,
    cooldown_until: DateTime<Utc>,
    comparison: ActionComparison,
) -> Result<UpdateStatement, MaintenanceError> {
    let (before_bytes, after_bytes, dead_tuples_before, dead_tuples_after) = match comparison {
        ActionComparison::Index { before_bytes, after_bytes } | ActionComparison::Rowstore { before_bytes, after_bytes } => (
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
        ActionComparison::Statistics { .. } | ActionComparison::Columnstore { .. } => (None, None, None, None),
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
        .and_where(branch_rows(target.branch()))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).eq(stored_target(target.target()).code()))
        .to_owned())
}

/// Drop a pending action whose live reading no longer qualifies, recording
/// `reason` as the decision. Nothing ran, so the cooldown and the last action
/// stay as they were; the sample under the threshold closes the breach run,
/// and the failures of the dropped action no longer count against a future
/// one.
pub(crate) async fn record_lapsed_action(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    target: BranchTarget,
    reason: DeferralReason,
) -> Result<(), MaintenanceError> {
    lapsed_action_update(tenant.key, target, reason).to_query().execute(connection).await?;
    Ok(())
}

/// Build the update dropping a lapsed pending action.
fn lapsed_action_update(tenant: TenantKey, target: BranchTarget, reason: DeferralReason) -> UpdateStatement {
    Query::update()
        .table(PostgresqlMaintenanceTargets::Table)
        .value(PostgresqlMaintenanceTargets::IsActionPending, false)
        .value(
            PostgresqlMaintenanceTargets::LastDecision,
            RecordedDecision::Policy(PolicyDecision::Deferred(reason)).code().code(),
        )
        .value(PostgresqlMaintenanceTargets::FirstBreachAt, Option::<DateTime<Utc>>::None)
        .value(PostgresqlMaintenanceTargets::BreachSamples, 0)
        .value(PostgresqlMaintenanceTargets::ConsecutiveFailures, 0)
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantKey).eq(tenant.0))
        .and_where(branch_rows(target.branch()))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).eq(stored_target(target.target()).code()))
        .to_owned()
}

/// Persist a failed action while keeping it pending for a retry once its
/// backoff ends.
pub(crate) async fn record_action_failure(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    target: BranchTarget,
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
        .and_where(branch_rows(target.branch()))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).eq(stored_target(target.target()).code()))
        .to_owned();
    statement.to_query().execute(connection).await?;
    Ok(())
}

/// Insert a target-state row if this is its first sample.
async fn ensure_target_state(connection: &mut PgConnection, tenant: &RegisteredTenant, target: BranchTarget) -> Result<(), MaintenanceError> {
    target_state_insert(tenant.key, target)?.to_query().execute(connection).await?;
    Ok(())
}

/// Build the idempotent target-state insert.
fn target_state_insert(tenant: TenantKey, target: BranchTarget) -> Result<InsertStatement, SeaQueryError> {
    let mut statement = Query::insert();
    statement
        .into_table(PostgresqlMaintenanceTargets::Table)
        .columns(TARGET_GRAIN)
        .values([
            tenant.0.into(),
            stored_snapshot_key(target.branch()).into(),
            stored_target(target.target()).code().into(),
        ])?
        .on_conflict(OnConflict::columns(TARGET_GRAIN).do_nothing().to_owned());
    Ok(statement.clone())
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        action_measurement::{ActionComparison, TupleCounts},
        action_policy::ActionVerdict,
        branch::{Branch, BranchTarget},
        candidate::{MaintenanceTarget, heap_target::HeapTarget, index_target::IndexTarget},
        error::MaintenanceError,
        policy::DeferralReason,
        state::{
            TenantKey,
            target_record::{action_completion_update, lapsed_action_update, target_state_insert},
        },
    };
    use chrono::Utc;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn live(target: MaintenanceTarget) -> BranchTarget {
        BranchTarget::new(Branch::Live, target).expect("the live branch holds every target")
    }

    #[test]
    fn a_vacuum_records_dead_tuples_and_clears_the_byte_pair() -> Result<(), MaintenanceError> {
        let comparison = ActionComparison::Heap {
            before: TupleCounts { dead: 50_000, live: 100_000 },
            after: TupleCounts { dead: 1_000, live: 100_000 },
        };
        let sql = action_completion_update(
            TenantKey(7),
            live(MaintenanceTarget::Heap(HeapTarget::CurrentAttributes)),
            ActionVerdict::Effective,
            Utc::now(),
            comparison,
        )?
        .to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""last_action_result" = 1"#), "{sql}");
        assert!(sql.contains(r#""last_dead_tuples_before" = 50000"#), "{sql}");
        assert!(sql.contains(r#""last_dead_tuples_after" = 1000"#), "{sql}");
        assert!(sql.contains(r#""tenant_key" = 7 AND "snapshot_key" = 0 AND "target" = 11"#), "{sql}");
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
            live(MaintenanceTarget::Index(IndexTarget::IdxCaValueNumber)),
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
            live(MaintenanceTarget::Heap(HeapTarget::CurrentAttributes)),
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

    #[test]
    fn a_lapsed_action_is_no_longer_pending_and_keeps_its_cooldown_and_last_action() {
        let sql = lapsed_action_update(
            TenantKey(7),
            live(MaintenanceTarget::Statistics(HeapTarget::Entities)),
            DeferralReason::BelowThreshold,
        )
        .to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""is_action_pending" = FALSE"#), "{sql}");
        assert!(sql.contains(r#""first_breach_at" = NULL"#), "{sql}");
        assert!(sql.contains(r#""breach_samples" = 0"#), "{sql}");
        assert!(sql.contains(r#""consecutive_failures" = 0"#), "{sql}");
        assert!(sql.contains(r#""tenant_key" = 7 AND "snapshot_key" = 0"#), "{sql}");
        for untouched in ["cooldown_until", "last_action_at", "last_action_result"] {
            assert!(!sql.contains(untouched), "{untouched} is left as it was: {sql}");
        }
    }

    #[test]
    fn a_live_target_row_is_inserted_with_key_zero_on_the_branch_grain() -> Result<(), MaintenanceError> {
        let sql = target_state_insert(TenantKey(7), live(MaintenanceTarget::Heap(HeapTarget::Entities)))?.to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"INSERT INTO "postgresql_maintenance_targets" ("tenant_key", "snapshot_key", "target") VALUES (7, 0, 12) ON CONFLICT ("tenant_key", "snapshot_key", "target") DO NOTHING"#
        );
        Ok(())
    }
}
