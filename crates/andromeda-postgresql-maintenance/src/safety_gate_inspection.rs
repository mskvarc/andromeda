//! Measuring the execution safety gates before a maintenance action.

use crate::{
    candidate::MaintenanceTarget,
    catalog_counter::{CatalogCounter, non_negative, percent},
    error::MaintenanceError,
    policy::SafetyGates,
    scalar_query::{select_bool, select_i64, select_optional_instant},
    vacuum_horizon,
};
use andromeda_postgresql_seaquery::{
    cast::cast,
    introspection::{current_database, current_setting},
    maintenance::{backend_pid, to_regclass},
    pg_type::PostgresType,
};
use chrono::Utc;
use sea_query::{Alias, Asterisk, Expr, ExprTrait, Func, Query, SelectStatement};
use sqlx::PgConnection;

/// Measure load, long transactions, the vacuum horizon of a heap target,
/// active maintenance and waiting locks.
///
/// Only a vacuum is gated on the horizon: a rebuild produces a compact index
/// whatever the horizon, so its holders are not read for an index target.
pub(crate) async fn safety_gates(connection: &mut PgConnection, target: MaintenanceTarget) -> Result<SafetyGates, MaintenanceError> {
    let heap = format!("public.{}", target.heap_name());
    let (relation, held_horizon) = match target {
        MaintenanceTarget::Index(index) => (Some(format!("public.{}", index.relation_name())), None),
        MaintenanceTarget::Heap(_) => (None, vacuum_horizon::horizon_held_by(&vacuum_horizon::horizon_holders(connection).await?)),
    };
    let active = select_i64(connection, active_backend_statement()).await?;
    let maximum = select_i64(connection, maximum_connections_statement()).await?;
    let oldest = select_optional_instant(connection, oldest_transaction_statement()).await?;
    let active_maintenance = select_bool(connection, active_maintenance_statement(&heap)).await?;
    let waiting_relation_lock = select_bool(connection, waiting_lock_statement(&heap, relation.as_deref())).await?;
    let oldest_transaction_seconds = oldest.map_or(0, |started| {
        let seconds = Utc::now().signed_duration_since(started).num_seconds();
        u64::try_from(seconds).unwrap_or(0)
    });
    let active = non_negative(active, CatalogCounter::ActiveBackends)?;
    let maximum = non_negative(maximum, CatalogCounter::MaximumConnections)?;
    Ok(SafetyGates {
        active_backend_percent: if maximum == 0 { 100 } else { percent(active, maximum) },
        oldest_transaction_seconds,
        held_horizon,
        active_maintenance,
        waiting_relation_lock,
    })
}

/// Active foreground backend count across the cluster.
///
/// CPU and I/O are shared by every database, so the count is cluster-wide.
/// Parallel workers carry foreground work; autovacuum workers, WAL senders and
/// background workers report `active` without competing with requests.
fn active_backend_statement() -> SelectStatement {
    Query::select()
        .expr(Func::count(Expr::col(Asterisk)))
        .from(Alias::new("pg_stat_activity"))
        .and_where(Expr::col(Alias::new("backend_type")).is_in(["client backend", "parallel worker"]))
        .and_where(Expr::col(Alias::new("state")).eq("active"))
        .and_where(Expr::col(Alias::new("pid")).ne(backend_pid()))
        .to_owned()
}

/// Oldest start of a client transaction holding this database's horizon.
///
/// The removable horizon of a non-shared relation depends only on its own
/// database, and `REINDEX CONCURRENTLY` waits only for snapshots there. A
/// writer idle in a read committed transaction holds no snapshot between
/// statements, but its transaction ID still holds the horizon and its locks
/// still block a concurrent rebuild.
fn oldest_transaction_statement() -> SelectStatement {
    Query::select()
        .expr(Func::min(Expr::col(Alias::new("xact_start"))))
        .from(Alias::new("pg_stat_activity"))
        .and_where(Expr::col(Alias::new("datname")).eq(current_database()))
        .and_where(Expr::col(Alias::new("backend_type")).eq("client backend"))
        .and_where(
            Expr::col(Alias::new("backend_xid"))
                .is_not_null()
                .or(Expr::col(Alias::new("backend_xmin")).is_not_null()),
        )
        .and_where(Expr::col(Alias::new("xact_start")).is_not_null())
        .and_where(Expr::col(Alias::new("pid")).ne(backend_pid()))
        .to_owned()
}

/// Configured maximum backend count.
fn maximum_connections_statement() -> SelectStatement {
    Query::select().expr(cast(current_setting("max_connections"), PostgresType::Bigint)).to_owned()
}

/// Whether PostgreSQL reports a build or vacuum on the relation.
fn active_maintenance_statement(relation: &str) -> SelectStatement {
    let build = Query::select()
        .expr(Expr::val(1))
        .from(Alias::new("pg_stat_progress_create_index"))
        .and_where(Expr::col(Alias::new("relid")).eq(to_regclass(Expr::val(relation))))
        .to_owned();
    let vacuum = Query::select()
        .expr(Expr::val(1))
        .from(Alias::new("pg_stat_progress_vacuum"))
        .and_where(Expr::col(Alias::new("relid")).eq(to_regclass(Expr::val(relation))))
        .to_owned();
    Query::select().expr(Expr::exists(build).or(Expr::exists(vacuum))).to_owned()
}

/// Whether another backend in this database waits for a relation lock.
///
/// `pg_locks.relation` is an OID unique only within one database, so the
/// lock's database is pinned to the current one.
fn waiting_lock_statement(heap: &str, index: Option<&str>) -> SelectStatement {
    let current_database_oid = Query::select()
        .column(Alias::new("oid"))
        .from(Alias::new("pg_database"))
        .and_where(Expr::col(Alias::new("datname")).eq(current_database()))
        .to_owned();
    let waiting = Query::select()
        .expr(Expr::val(1))
        .from(Alias::new("pg_locks"))
        .and_where(Expr::col(Alias::new("database")).in_subquery(current_database_oid))
        .and_where(match index {
            Some(index) => Expr::col(Alias::new("relation")).is_in([to_regclass(Expr::val(heap)), to_regclass(Expr::val(index))]),
            None => Expr::col(Alias::new("relation")).eq(to_regclass(Expr::val(heap))),
        })
        .and_where(Expr::col(Alias::new("granted")).eq(false))
        .to_owned();
    Query::select().expr(Expr::exists(waiting)).to_owned()
}

#[cfg(test)]
mod tests {
    use crate::safety_gate_inspection::{active_backend_statement, oldest_transaction_statement, waiting_lock_statement};
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn active_backend_gate_counts_foreground_backends_only() {
        let sql = active_backend_statement().to_string(PostgresQueryBuilder);
        assert!(sql.contains("\"backend_type\" IN ('client backend', 'parallel worker')"), "{sql}");
        assert!(!sql.contains("datname"), "{sql}");
    }

    #[test]
    fn long_transaction_gate_is_scoped_to_the_current_database_and_horizon_holders() {
        let sql = oldest_transaction_statement().to_string(PostgresQueryBuilder);
        assert!(sql.contains("\"datname\" = current_database()"), "{sql}");
        assert!(sql.contains("\"backend_type\" = 'client backend'"), "{sql}");
        assert!(sql.contains("\"backend_xid\" IS NOT NULL OR \"backend_xmin\" IS NOT NULL"), "{sql}");
    }

    #[test]
    fn lock_gate_is_scoped_to_the_current_database() {
        let sql = waiting_lock_statement("public.current_attributes", Some("public.idx_ca_value_string")).to_string(PostgresQueryBuilder);
        assert!(
            sql.contains("\"database\" IN (SELECT \"oid\" FROM \"pg_database\" WHERE \"datname\" = current_database())"),
            "{sql}"
        );
        assert!(sql.contains("\"granted\" = FALSE"), "{sql}");
    }
}
