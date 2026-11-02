//! Which queued action a tenant's visit runs next.
//!
//! A qualified action stays pending on its target row until it completes. A
//! visit runs one: any due target before a GIN its canary queued, so the
//! canary is rebuilt on an earlier pass, and a target in its failure backoff
//! is skipped without holding back the rest. A canary and its GIN live in the
//! same branch, so the pending actions of each branch are ordered on their own.
//!
//! A branch decided under observe mode keeps no queued work: observe mode
//! never queues, and an action queued before automatic mode lost the branch's
//! `MAINTAIN` would otherwise be reached on every visit and end it before its
//! index probes. Such a branch's queued actions are withdrawn instead, and
//! queued again by a later sample once the branch is acted in. An action whose
//! own right the role lacks, beyond the branch's `MAINTAIN`, is withdrawn the
//! same way, alone.

use crate::{
    branch::{Branch, BranchTarget},
    candidate::MaintenanceTarget,
    error::MaintenanceError,
    evidence::{
        recorded_decision::RecordedDecision,
        target_code::{stored_target, target_of},
    },
    policy::PolicyDecision,
    state::{
        RegisteredTenant,
        TenantKey,
        branch_rows::{branch_of_stored_key, branch_rows},
    },
};
use andromeda_postgresql_codes::maintenance_target::MaintenanceTargetCode;
use andromeda_postgresql_schema::maintenance::PostgresqlMaintenanceTargets;
use andromeda_postgresql_seaquery::statement::Statement;
use chrono::{DateTime, Utc};
use sea_query::{Expr, ExprTrait, Iden, Order, Query, SelectStatement, UpdateStatement};
use sqlx::{PgConnection, Row};

/// One queued action of one tenant's branch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingRow {
    /// The tenant.
    tenant: TenantKey,
    /// The branch whose target row holds the action.
    branch: Branch,
    /// The action.
    action: PendingAction,
}

/// One queued action and the cooldown its last attempt set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingAction {
    /// The queued target.
    target: MaintenanceTarget,
    /// Backoff deadline of a failed attempt, absent before any attempt.
    cooldown_until: Option<DateTime<Utc>>,
}

/// Every action queued in `branches` at this instant: the work queued on an
/// earlier pass, which a visit may run once its samples are recorded.
pub(crate) async fn queued_actions(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    branches: impl IntoIterator<Item = Branch>,
) -> Result<Vec<BranchTarget>, MaintenanceError> {
    let mut queued = Vec::new();
    for branch in branches {
        for action in branch_pending_actions(connection, tenant, branch).await? {
            queued.push(BranchTarget::new(branch, action.target)?);
        }
    }
    Ok(queued)
}

/// The first action of `branches` still pending among `queued`, searched in
/// order, each branch's chosen by [`first_pending`]. An action queued since
/// `queued` was read is not chosen and still holds back a GIN its canary
/// queued.
pub(crate) async fn first_branch_action(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    branches: impl IntoIterator<Item = Branch>,
    queued: &[BranchTarget],
) -> Result<Option<BranchTarget>, MaintenanceError> {
    let now = Utc::now();
    for branch in branches {
        let pending = branch_pending_actions(connection, tenant, branch).await?;
        let eligible = |target| queued.iter().any(|row| row.branch() == branch && row.target() == target);
        if let Some(target) = first_pending(&pending, now, eligible) {
            return Ok(Some(BranchTarget::new(branch, target)?));
        }
    }
    Ok(None)
}

/// Every pending action of `tenant`'s `branch`, in target order.
async fn branch_pending_actions(connection: &mut PgConnection, tenant: &RegisteredTenant, branch: Branch) -> Result<Vec<PendingAction>, MaintenanceError> {
    let statement = pending_actions_statement()
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantKey).eq(tenant.key.0))
        .and_where(branch_rows(branch))
        .to_owned();
    let pending = read_pending_actions(connection, &statement).await?;
    Ok(pending.into_iter().map(|row| row.action).collect())
}

/// Withdraw every action queued in `branch`, recording each as one automatic
/// mode would act on, and return the targets withdrawn.
pub(crate) async fn withdraw_actions(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    branch: Branch,
) -> Result<Vec<MaintenanceTarget>, MaintenanceError> {
    withdrawn_targets(connection, &withdraw_actions_update(tenant.key, branch)).await
}

/// Withdraw the action queued on `row`, recording it as one automatic mode
/// would act on, and return the target if it was queued.
pub(crate) async fn withdraw_action(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    row: BranchTarget,
) -> Result<Vec<MaintenanceTarget>, MaintenanceError> {
    withdrawn_targets(connection, &withdraw_action_update(tenant.key, row)).await
}

/// Run a withdrawal and decode the targets it cleared.
async fn withdrawn_targets(connection: &mut PgConnection, statement: &UpdateStatement) -> Result<Vec<MaintenanceTarget>, MaintenanceError> {
    let rows = statement.to_query().fetch_all(connection).await?;
    rows.iter()
        .map(|row| {
            Ok(target_of(MaintenanceTargetCode::try_from(
                row.try_get::<i16, _>(PostgresqlMaintenanceTargets::Target.to_string().as_str())?,
            )?))
        })
        .collect()
}

/// Build the update clearing every pending action of `tenant`'s `branch`. A
/// withdrawn action has no retry left to back off from, and its breach run
/// stays: the problem it was queued for is still sampled.
fn withdraw_actions_update(tenant: TenantKey, branch: Branch) -> UpdateStatement {
    Query::update()
        .table(PostgresqlMaintenanceTargets::Table)
        .value(PostgresqlMaintenanceTargets::IsActionPending, false)
        .value(
            PostgresqlMaintenanceTargets::LastDecision,
            RecordedDecision::Policy(PolicyDecision::WouldAct).code().code(),
        )
        .value(PostgresqlMaintenanceTargets::ConsecutiveFailures, 0)
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantKey).eq(tenant.0))
        .and_where(branch_rows(branch))
        .and_where(Expr::col(PostgresqlMaintenanceTargets::IsActionPending).eq(true))
        .returning_col(PostgresqlMaintenanceTargets::Target)
        .to_owned()
}

/// Build the update clearing the pending action of `tenant`'s `row` alone.
fn withdraw_action_update(tenant: TenantKey, row: BranchTarget) -> UpdateStatement {
    withdraw_actions_update(tenant, row.branch())
        .and_where(Expr::col(PostgresqlMaintenanceTargets::Target).eq(stored_target(row.target()).code()))
        .to_owned()
}

/// Every tenant to which [`first_pending`] would give an action of some
/// branch now.
pub(crate) async fn tenants_with_due_actions(connection: &mut PgConnection, now: DateTime<Utc>) -> Result<Vec<TenantKey>, MaintenanceError> {
    let pending = read_pending_actions(connection, &pending_actions_statement()).await?;
    Ok(due_tenants(&pending, now))
}

/// The tenants of `pending`, ordered by tenant and branch, with a due action
/// in at least one branch, each once. Each branch's actions are ordered on
/// their own, so a canary backing off in one branch never holds back a GIN of
/// another.
fn due_tenants(pending: &[PendingRow], now: DateTime<Utc>) -> Vec<TenantKey> {
    let mut due = pending
        .chunk_by(|left, right| (left.tenant, left.branch) == (right.tenant, right.branch))
        .filter_map(|branch_pending| {
            let actions = branch_pending.iter().map(|row| row.action).collect::<Vec<_>>();
            first_pending(&actions, now, |_| true).and(branch_pending.first().map(|row| row.tenant))
        })
        .collect::<Vec<_>>();
    due.dedup();
    due
}

/// Every pending action, grouped by tenant and branch and ordered by target
/// code.
fn pending_actions_statement() -> SelectStatement {
    Query::select()
        .columns([
            PostgresqlMaintenanceTargets::TenantKey,
            PostgresqlMaintenanceTargets::SnapshotKey,
            PostgresqlMaintenanceTargets::Target,
            PostgresqlMaintenanceTargets::CooldownUntil,
        ])
        .from(PostgresqlMaintenanceTargets::Table)
        .and_where(Expr::col(PostgresqlMaintenanceTargets::IsActionPending).eq(true))
        .order_by(PostgresqlMaintenanceTargets::TenantKey, Order::Asc)
        .order_by(PostgresqlMaintenanceTargets::SnapshotKey, Order::Asc)
        .order_by(PostgresqlMaintenanceTargets::Target, Order::Asc)
        .to_owned()
}

/// Run a pending-action statement and decode each row with its tenant and
/// branch.
async fn read_pending_actions(connection: &mut PgConnection, statement: &SelectStatement) -> Result<Vec<PendingRow>, MaintenanceError> {
    let rows = statement.to_query().fetch_all(connection).await?;
    rows.iter()
        .map(|row| {
            let snapshot_key = row.try_get::<i64, _>(PostgresqlMaintenanceTargets::SnapshotKey.to_string().as_str())?;
            Ok(PendingRow {
                tenant: TenantKey(row.try_get(PostgresqlMaintenanceTargets::TenantKey.to_string().as_str())?),
                branch: branch_of_stored_key(snapshot_key)?,
                action: PendingAction {
                    target: target_of(MaintenanceTargetCode::try_from(
                        row.try_get::<i16, _>(PostgresqlMaintenanceTargets::Target.to_string().as_str())?,
                    )?),
                    cooldown_until: row.try_get(PostgresqlMaintenanceTargets::CooldownUntil.to_string().as_str())?,
                },
            })
        })
        .collect()
}

/// The pending target to act on first among those `eligible` admits: any due
/// target but a GIN queued through a canary, so a canary is rebuilt on an
/// earlier pass than its GIN, and a due canary-queued GIN once nothing else
/// is pending. A target still in its failure backoff, or one `eligible`
/// refuses, is skipped, so it does not hold back the rest of the tenant's
/// pass, yet it still holds back every canary-queued GIN.
fn first_pending(pending: &[PendingAction], now: DateTime<Utc>, eligible: impl Fn(MaintenanceTarget) -> bool) -> Option<MaintenanceTarget> {
    let is_gin = |action: &PendingAction| match action.target {
        MaintenanceTarget::Index(index) => index.canary().is_some(),
        MaintenanceTarget::Heap(_) | MaintenanceTarget::Statistics(_) | MaintenanceTarget::Columnstore(_) | MaintenanceTarget::Rowstore(_) => false,
    };
    let is_ready = |action: &PendingAction| action.cooldown_until.is_none_or(|until| until <= now) && eligible(action.target);
    if pending.iter().all(is_gin) {
        pending.iter().find(|action| is_ready(action))
    } else {
        pending.iter().find(|action| !is_gin(action) && is_ready(action))
    }
    .map(|action| action.target)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        branch::{Branch, BranchTarget},
        candidate::{MaintenanceTarget, heap_target::HeapTarget, history_table::HistoryTable, index_target::IndexTarget},
        state::{
            TenantKey,
            pending_action::{
                PendingAction,
                PendingRow,
                due_tenants,
                first_pending,
                pending_actions_statement,
                withdraw_action_update,
                withdraw_actions_update,
            },
        },
    };
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use chrono::{DateTime, Duration, Utc};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

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

    /// Every target is eligible.
    const fn any(_: MaintenanceTarget) -> bool {
        true
    }

    #[test]
    fn an_action_queued_since_the_visit_began_is_not_chosen() {
        let lapsed = MaintenanceTarget::Statistics(HeapTarget::Entities);
        let stale = MaintenanceTarget::Statistics(HeapTarget::TemporalAttributeStreams);
        let new = MaintenanceTarget::Heap(HeapTarget::CurrentAttributes);
        let queued_before = |target| target == lapsed || target == stale;
        assert_eq!(first_pending(&[due(new), due(stale)], Utc::now(), queued_before), Some(stale));
        assert_eq!(first_pending(&[due(new)], Utc::now(), queued_before), None);
    }

    #[test]
    fn a_canary_queued_since_the_visit_began_still_holds_back_its_gin() {
        let canary = MaintenanceTarget::Index(IndexTarget::IdxCaValueString);
        let gin = MaintenanceTarget::Index(IndexTarget::IdxCaValueStringTrgm);
        assert_eq!(first_pending(&[due(canary), due(gin)], Utc::now(), |target| target == gin), None);
    }

    #[test]
    fn a_canary_is_acted_on_before_the_gin_it_queued() {
        let canary = MaintenanceTarget::Index(IndexTarget::IdxCaValueString);
        let gin = MaintenanceTarget::Index(IndexTarget::IdxCaValueStringTrgm);
        assert_eq!(first_pending(&[due(gin), due(canary)], Utc::now(), any), Some(canary));
    }

    #[test]
    fn a_gin_is_acted_on_once_nothing_else_is_pending() {
        assert_eq!(
            first_pending(&[due(MaintenanceTarget::Index(IndexTarget::IdxCaValueStringTrgm))], Utc::now(), any),
            Some(MaintenanceTarget::Index(IndexTarget::IdxCaValueStringTrgm))
        );
        assert_eq!(first_pending(&[], Utc::now(), any), None);
    }

    #[test]
    fn a_pending_heap_goes_ahead_of_a_gin() {
        assert_eq!(
            first_pending(
                &[
                    due(MaintenanceTarget::Index(IndexTarget::IdxCaValueStringTrgm)),
                    due(MaintenanceTarget::Heap(HeapTarget::CurrentAttributes)),
                ],
                Utc::now(),
                any
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
        assert_eq!(first_pending(&[due(gin), due(heap)], Utc::now(), any), Some(gin));
    }

    /// Tenant 1's live canary backs off, which holds back its live GIN, while
    /// Snapshot 7's GIN has nothing ahead of it in its own branch. Tenant 2's
    /// only action backs off.
    #[test]
    fn a_due_action_in_any_branch_ranks_its_tenant_once() {
        let now = Utc::now();
        let snapshot = Branch::Snapshot(SnapshotKey::try_from(7).expect("positive snapshot key"));
        let canary = MaintenanceTarget::Index(IndexTarget::IdxCaValueString);
        let gin = MaintenanceTarget::Index(IndexTarget::IdxCaValueStringTrgm);
        let row = |tenant: i64, branch: Branch, action: PendingAction| PendingRow {
            tenant: TenantKey(tenant),
            branch,
            action,
        };
        let held_back = [row(1, Branch::Live, backing_off(canary, now)), row(1, Branch::Live, due(gin))];
        assert_eq!(due_tenants(&held_back, now), []);
        let pending = [
            row(1, Branch::Live, backing_off(canary, now)),
            row(1, Branch::Live, due(gin)),
            row(1, snapshot, due(gin)),
            row(2, Branch::Live, backing_off(canary, now)),
        ];
        assert_eq!(due_tenants(&pending, now), [TenantKey(1)]);
        let both = [row(1, Branch::Live, due(canary)), row(1, snapshot, due(gin))];
        assert_eq!(due_tenants(&both, now), [TenantKey(1)]);
    }

    #[test]
    fn pending_actions_are_read_in_tenant_then_branch_order() {
        let sql = pending_actions_statement().to_string(PostgresQueryBuilder);
        assert!(sql.ends_with(r#"ORDER BY "tenant_key" ASC, "snapshot_key" ASC, "target" ASC"#), "{sql}");
        assert!(!sql.contains(r#""snapshot_key" ="#), "every branch is read: {sql}");
    }

    /// Decision 3 is would-act.
    #[test]
    fn withdrawing_clears_only_the_pending_actions_of_one_branch() {
        let snapshot = Branch::Snapshot(SnapshotKey::try_from(7).expect("positive snapshot key"));
        assert_eq!(
            withdraw_actions_update(TenantKey(4), snapshot).to_string(PostgresQueryBuilder),
            r#"UPDATE "postgresql_maintenance_targets" SET "is_action_pending" = FALSE, "last_decision" = 3, "consecutive_failures" = 0 WHERE "tenant_key" = 4 AND "snapshot_key" = 7 AND "is_action_pending" = TRUE RETURNING "target""#
        );
        let live = withdraw_actions_update(TenantKey(4), Branch::Live).to_string(PostgresQueryBuilder);
        assert!(live.contains(r#""snapshot_key" = 0 AND"#), "{live}");
    }

    /// Target code 35 is `attribute_history_columnstore`.
    #[test]
    fn withdrawing_one_action_clears_only_that_target_of_its_branch() {
        let row = BranchTarget::new(Branch::Live, MaintenanceTarget::Columnstore(HistoryTable::AttributeHistory)).expect("live branch holds the hypertable");
        assert_eq!(
            withdraw_action_update(TenantKey(4), row).to_string(PostgresQueryBuilder),
            r#"UPDATE "postgresql_maintenance_targets" SET "is_action_pending" = FALSE, "last_decision" = 3, "consecutive_failures" = 0 WHERE "tenant_key" = 4 AND "snapshot_key" = 0 AND "is_action_pending" = TRUE AND "target" = 35 RETURNING "target""#
        );
    }

    #[test]
    fn a_backing_off_action_is_skipped_and_still_holds_back_its_gin() {
        let now = Utc::now();
        let canary = MaintenanceTarget::Index(IndexTarget::IdxCaValueString);
        let gin = MaintenanceTarget::Index(IndexTarget::IdxCaValueStringTrgm);
        let heap = MaintenanceTarget::Heap(HeapTarget::Entities);
        assert_eq!(first_pending(&[backing_off(canary, now), due(heap)], now, any), Some(heap));
        assert_eq!(first_pending(&[backing_off(canary, now), due(gin)], now, any), None);
        assert_eq!(first_pending(&[backing_off(gin, now)], now, any), None);
        assert_eq!(first_pending(&[backing_off(canary, now)], now + Duration::minutes(15), any), Some(canary));
    }
}
