//! Which queued action a tenant's visit runs next.
//!
//! A qualified action stays pending on its target row until it completes. A
//! visit runs one: any due target before a GIN its canary queued, so the
//! canary is rebuilt on an earlier pass, and a target in its failure backoff
//! is skipped without holding back the rest.

use crate::{
    branch::Branch,
    candidate::MaintenanceTarget,
    error::MaintenanceError,
    evidence::target_code::target_of,
    state::{RegisteredTenant, TenantKey, branch_rows::branch_rows},
};
use andromeda_postgresql_codes::maintenance_target::MaintenanceTargetCode;
use andromeda_postgresql_schema::maintenance::PostgresqlMaintenanceTargets;
use chrono::{DateTime, Utc};
use sea_query::{Expr, ExprTrait, Iden, Order, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgConnection, Row};

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
    let pending = pending.into_iter().map(|(_, action)| action).collect::<Vec<_>>();
    Ok(first_pending(&pending, Utc::now()))
}

/// Every tenant that [`pending_action`] would give a live-branch action to
/// now. Each branch's pending actions are ordered on their own, so the rows of
/// one branch are never mixed with another's.
pub(crate) async fn tenants_with_due_actions(connection: &mut PgConnection, now: DateTime<Utc>) -> Result<Vec<TenantKey>, MaintenanceError> {
    let statement = pending_actions_statement().and_where(branch_rows(Branch::Live)).to_owned();
    let pending = read_pending_actions(connection, &statement).await?;
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

#[cfg(test)]
mod tests {
    use crate::{
        candidate::{HeapTarget, IndexTarget, MaintenanceTarget},
        state::pending_action::{PendingAction, first_pending},
    };
    use chrono::{DateTime, Duration, Utc};
    use pretty_assertions::assert_eq;

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
