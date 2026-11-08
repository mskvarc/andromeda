//! Measuring the execution safety gates before a maintenance action.

use crate::{
    branch::{Branch, BranchTarget},
    candidate::{MaintenanceTarget, history_table::HistoryTable},
    catalog_counter::{CatalogCounter, non_negative, percent},
    columnstore_partiality::HistoryChunk,
    error::MaintenanceError,
    policy::SafetyGates,
    vacuum_horizon::{self, HorizonScope},
};
use andromeda_postgresql_seaquery::{
    cast::cast,
    introspection::{current_database, current_setting},
    maintenance::{backend_pid, to_regclass},
    pg_type::PostgresType,
    repack::REPACK_TRIGGER,
    setting::Setting,
    statement::Statement,
};
use chrono::{DateTime, Utc};
use sea_query::{Alias, Asterisk, Expr, ExprTrait, Func, Query, SelectStatement};
use sqlx::PgConnection;

/// The relations an action's gates read.
#[derive(Clone, Copy, Debug)]
pub(crate) enum GateScope<'c> {
    /// A fixed target in its branch: an index with its heap, a heap, a heap's
    /// statistics, or a whole history hypertable.
    Target(BranchTarget),
    /// The one chunk of a history hypertable an action works on.
    Chunk {
        /// The hypertable.
        table: HistoryTable,
        /// The chunk.
        chunk: &'c HistoryChunk,
    },
}

/// Measure load, long transactions, the vacuum horizon of a heap target,
/// and active maintenance and waiting locks on the relations of `scope`.
///
/// Only a vacuum is gated on the horizon: a rebuild produces a compact index,
/// an analyze samples live rows whatever the horizon, and a recompression
/// rewrites the live rows of the segments it touches, so its holders are read
/// for none of them. A chunk is gated on its own operations and locks and on
/// waiting locks of its hypertable.
pub(crate) async fn safety_gates(connection: &mut PgConnection, scope: GateScope<'_>) -> Result<SafetyGates, MaintenanceError> {
    let (heap, relation, held_horizon) = match scope {
        GateScope::Target(row) => {
            let branch = row.branch();
            match row.target() {
                MaintenanceTarget::Index(index) => (
                    branch.qualified(index.heap().relation_name()),
                    Some(branch.qualified(index.relation_name())),
                    None,
                ),
                MaintenanceTarget::Heap(target) => (
                    branch.qualified(target.relation_name()),
                    None,
                    vacuum_horizon::horizon_held_by(&vacuum_horizon::horizon_holders(connection, HorizonScope::TenantDatabase).await?),
                ),
                MaintenanceTarget::Statistics(target) | MaintenanceTarget::Repack(target) => (branch.qualified(target.relation_name()), None, None),
                MaintenanceTarget::Columnstore(table) | MaintenanceTarget::Rowstore(table) => (branch.qualified(table.relation_name()), None, None),
            }
        }
        GateScope::Chunk { table, chunk } => (
            chunk.relation.clone(), // clone: the chunk is borrowed, so its relation name is copied into the owned tuple
            Some(Branch::Live.qualified(table.relation_name())),
            None,
        ),
    };
    let active = active_backend_statement().to_query_scalar::<i64>().fetch_one(&mut *connection).await?;
    let maximum = maximum_connections_statement().to_query_scalar::<i64>().fetch_one(&mut *connection).await?;
    let oldest = oldest_transaction_statement()
        .to_query_scalar::<Option<DateTime<Utc>>>()
        .fetch_one(&mut *connection)
        .await?;
    let active_maintenance = active_maintenance_statement(&heap)
        .to_query_scalar::<bool>()
        .fetch_one(&mut *connection)
        .await?;
    let waiting_relation_lock = waiting_lock_statement(&heap, relation.as_deref())
        .to_query_scalar::<bool>()
        .fetch_one(&mut *connection)
        .await?;
    let repack_running = repack_trigger_statement(&heap).to_query_scalar::<bool>().fetch_one(&mut *connection).await?;
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
        repack_running,
    })
}

/// Whether a repack's trigger is installed on `heap`: a repack of it runs, or
/// one that was interrupted left its trigger behind.
fn repack_trigger_statement(heap: &str) -> SelectStatement {
    let trigger = Query::select()
        .expr(Expr::val(1))
        .from(Alias::new("pg_trigger"))
        .and_where(Expr::col(Alias::new("tgrelid")).eq(to_regclass(Expr::val(heap))))
        .and_where(Expr::col(Alias::new("tgname")).eq(REPACK_TRIGGER))
        .to_owned();
    Query::select().expr(Expr::exists(trigger)).to_owned()
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
    Query::select()
        .expr(cast(current_setting(Setting::MaxConnections), PostgresType::Bigint))
        .to_owned()
}

/// Whether PostgreSQL reports a build, vacuum or analyze on the heap in this
/// database. Each takes the heap's `SHARE UPDATE EXCLUSIVE` lock, so any of
/// them blocks every maintenance action on it.
///
/// A progress view lists every database's operations and `relid` is an OID
/// unique only within one database, so the progress row must also name this
/// database.
fn active_maintenance_statement(relation: &str) -> SelectStatement {
    let reported = |view: &'static str| {
        Expr::exists(
            Query::select()
                .expr(Expr::val(1))
                .from(Alias::new(view))
                .and_where(Expr::col(Alias::new("relid")).eq(to_regclass(Expr::val(relation))))
                .and_where(Expr::col(Alias::new("datname")).eq(current_database()))
                .to_owned(),
        )
    };
    Query::select()
        .expr(
            reported("pg_stat_progress_create_index")
                .or(reported("pg_stat_progress_vacuum"))
                .or(reported("pg_stat_progress_analyze")),
        )
        .to_owned()
}

/// Whether another backend in this database waits for a lock on `heap` or on
/// the second relation of the action, its index or its chunk's hypertable.
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
    use crate::safety_gate_inspection::{
        active_backend_statement,
        active_maintenance_statement,
        oldest_transaction_statement,
        repack_trigger_statement,
        waiting_lock_statement,
    };
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
    fn active_maintenance_gate_reads_builds_vacuums_and_analyzes_of_the_current_database() {
        let sql = active_maintenance_statement("public.entity_search").to_string(PostgresQueryBuilder);
        for view in ["pg_stat_progress_create_index", "pg_stat_progress_vacuum", "pg_stat_progress_analyze"] {
            assert!(
                sql.contains(&format!(
                    "EXISTS(SELECT 1 FROM \"{view}\" WHERE \"relid\" = to_regclass('public.entity_search') AND \"datname\" = current_database())"
                )),
                "{view}: {sql}"
            );
        }
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

    #[test]
    fn the_repack_gate_reads_the_trigger_a_run_installs_on_the_heap() {
        assert_eq!(
            repack_trigger_statement("public.notification_deliveries").to_string(PostgresQueryBuilder),
            r#"SELECT EXISTS(SELECT 1 FROM "pg_trigger" WHERE "tgrelid" = to_regclass('public.notification_deliveries') AND "tgname" = 'repack_trigger')"#
        );
    }
}
