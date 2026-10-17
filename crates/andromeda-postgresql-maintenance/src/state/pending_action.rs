//! Which queued action a tenant's visit runs next.
//!
//! A qualified action stays pending on its target row until it completes. A
//! visit runs one: any due target before a GIN its canary queued, so the
//! canary is rebuilt on an earlier pass, and a target in its failure backoff
//! is skipped without holding back the rest. A canary and its GIN live in the
//! same branch, so the pending actions of each branch are ordered on their own.

use crate::{
    branch::{Branch, BranchTarget},
    candidate::MaintenanceTarget,
    error::MaintenanceError,
    evidence::target_code::target_of,
    state::{RegisteredTenant, TenantKey, branch_rows::branch_rows},
};
use andromeda_context_space::snapshot_key::SnapshotKey;
use andromeda_postgresql_codes::maintenance_target::MaintenanceTargetCode;
use andromeda_postgresql_schema::maintenance::PostgresqlMaintenanceTargets;
use chrono::{DateTime, Utc};
use sea_query::{Expr, ExprTrait, Iden, NullOrdering, Order, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
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

/// Return the first pending action of `branch` whose backoff has elapsed,
/// keeping the canary B-tree ahead of its GIN pair.
pub(crate) async fn pending_action(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    branch: Branch,
) -> Result<Option<MaintenanceTarget>, MaintenanceError> {
    let statement = pending_actions_statement()
        .and_where(Expr::col(PostgresqlMaintenanceTargets::TenantKey).eq(tenant.key.0))
        .and_where(branch_rows(branch))
        .to_owned();
    let pending = read_pending_actions(connection, &statement).await?;
    let pending = pending.into_iter().map(|row| row.action).collect::<Vec<_>>();
    Ok(first_pending(&pending, Utc::now()))
}

/// The first pending action of `branches`, searched in order, each branch's
/// chosen by [`pending_action`].
pub(crate) async fn first_branch_action(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    branches: &[Branch],
) -> Result<Option<BranchTarget>, MaintenanceError> {
    for &branch in branches {
        if let Some(target) = pending_action(connection, tenant, branch).await? {
            return Ok(Some(BranchTarget::new(branch, target)?));
        }
    }
    Ok(None)
}

/// Every tenant to which [`pending_action`] would give an action of some
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
            first_pending(&actions, now).and(branch_pending.first().map(|row| row.tenant))
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
        .order_by_with_nulls(PostgresqlMaintenanceTargets::SnapshotKey, Order::Asc, NullOrdering::First)
        .order_by(PostgresqlMaintenanceTargets::Target, Order::Asc)
        .to_owned()
}

/// Run a pending-action statement and decode each row with its tenant and
/// branch.
async fn read_pending_actions(connection: &mut PgConnection, statement: &SelectStatement) -> Result<Vec<PendingRow>, MaintenanceError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(connection).await?;
    rows.iter()
        .map(|row| {
            let snapshot_key = row.try_get::<Option<i64>, _>(PostgresqlMaintenanceTargets::SnapshotKey.to_string().as_str())?;
            Ok(PendingRow {
                tenant: TenantKey(row.try_get(PostgresqlMaintenanceTargets::TenantKey.to_string().as_str())?),
                branch: snapshot_key.map(SnapshotKey::try_from).transpose()?.map_or(Branch::Live, Branch::Snapshot),
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

/// The pending target to act on first: any due target but a GIN queued
/// through a canary, so a canary is rebuilt on an earlier pass than its GIN,
/// and a due canary-queued GIN once nothing else is pending. A target still in its failure
/// backoff is skipped, so it does not hold back the rest of the tenant's pass,
/// yet it still holds back every canary-queued GIN.
fn first_pending(pending: &[PendingAction], now: DateTime<Utc>) -> Option<MaintenanceTarget> {
    let is_gin = |action: &PendingAction| match action.target {
        MaintenanceTarget::Index(index) => index.canary().is_some(),
        MaintenanceTarget::Heap(_) | MaintenanceTarget::Statistics(_) | MaintenanceTarget::Columnstore(_) => false,
    };
    let is_due = |action: &PendingAction| action.cooldown_until.is_none_or(|until| until <= now);
    if pending.iter().all(is_gin) {
        pending.iter().find(|action| is_due(action))
    } else {
        pending.iter().find(|action| !is_gin(action) && is_due(action))
    }
    .map(|action| action.target)
}

#[cfg(test)]
mod tests {
    use crate::{
        branch::Branch,
        candidate::{MaintenanceTarget, heap_target::HeapTarget, index_target::IndexTarget},
        state::{
            TenantKey,
            pending_action::{PendingAction, PendingRow, due_tenants, first_pending, pending_actions_statement},
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
        assert!(
            sql.ends_with(r#"ORDER BY "tenant_key" ASC, "snapshot_key" ASC NULLS FIRST, "target" ASC"#),
            "{sql}"
        );
        assert!(!sql.contains(r#""snapshot_key" IS NULL"#), "every branch is read: {sql}");
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
