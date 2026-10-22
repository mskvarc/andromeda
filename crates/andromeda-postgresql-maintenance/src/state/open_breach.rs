//! The open breach runs that triage waits on.
//!
//! A breach run qualifies only on a sample taken once it has lasted the
//! profile's qualifying duration. A tenant whose writes stop after the run
//! opened shows no churn, so triage reads each tenant's oldest open run to
//! visit it when that run is ready to qualify.

use crate::{error::MaintenanceError, state::TenantKey};
use andromeda_postgresql_schema::maintenance::PostgresqlMaintenanceTargets;
use andromeda_postgresql_seaquery::statement::Statement;
use chrono::{DateTime, Utc};
use sea_query::{Expr, ExprTrait, Func, Iden, Order, Query, SelectStatement};
use sqlx::{PgConnection, Row};

/// The oldest open breach run of one tenant's targets, in any branch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OpenBreach {
    /// The tenant.
    pub(crate) tenant: TenantKey,
    /// When the run's first breaching sample was taken.
    pub(crate) since: DateTime<Utc>,
}

/// Every tenant with an open breach run on a target without a queued action,
/// paired with its oldest run's start. A queued action reaches triage as due
/// work instead.
pub(crate) async fn open_breaches(connection: &mut PgConnection) -> Result<Vec<OpenBreach>, MaintenanceError> {
    let rows = open_breaches_statement().to_query().fetch_all(connection).await?;
    rows.iter()
        .map(|row| {
            Ok(OpenBreach {
                tenant: TenantKey(row.try_get(PostgresqlMaintenanceTargets::TenantKey.to_string().as_str())?),
                since: row.try_get(PostgresqlMaintenanceTargets::FirstBreachAt.to_string().as_str())?,
            })
        })
        .collect()
}

/// The oldest `first_breach_at` per tenant over the target rows with an open
/// run and no queued action.
fn open_breaches_statement() -> SelectStatement {
    Query::select()
        .column(PostgresqlMaintenanceTargets::TenantKey)
        .expr_as(
            Func::min(Expr::col(PostgresqlMaintenanceTargets::FirstBreachAt)),
            PostgresqlMaintenanceTargets::FirstBreachAt,
        )
        .from(PostgresqlMaintenanceTargets::Table)
        .and_where(Expr::col(PostgresqlMaintenanceTargets::FirstBreachAt).is_not_null())
        .and_where(Expr::col(PostgresqlMaintenanceTargets::IsActionPending).eq(false))
        .group_by_col(PostgresqlMaintenanceTargets::TenantKey)
        .order_by(PostgresqlMaintenanceTargets::TenantKey, Order::Asc)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::state::open_breach::open_breaches_statement;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn open_breaches_read_the_oldest_run_per_tenant_without_a_queued_action() {
        assert_eq!(
            open_breaches_statement().to_string(PostgresQueryBuilder),
            "SELECT \"tenant_key\", MIN(\"first_breach_at\") AS \"first_breach_at\" FROM \"postgresql_maintenance_targets\" WHERE \
             \"first_breach_at\" IS NOT NULL AND \"is_action_pending\" = FALSE GROUP BY \"tenant_key\" ORDER BY \"tenant_key\" ASC"
        );
    }
}
