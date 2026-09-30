//! Measuring the execution safety gates before a maintenance action.

use crate::{
    candidate::MaintenanceTarget,
    catalog_counter::{CatalogCounter, non_negative, percent},
    error::MaintenanceError,
    policy::SafetyGates,
    scalar_query::{select_bool, select_i64, select_optional_instant},
};
use andromeda_postgresql_seaquery::{
    cast::cast,
    introspection::current_setting,
    maintenance::{backend_pid, to_regclass},
    pg_type::PostgresType,
};
use chrono::Utc;
use sea_query::{Alias, Asterisk, Expr, ExprTrait, Func, Query, SelectStatement};
use sqlx::PgConnection;

/// Measure load, long transactions, active maintenance and waiting locks.
pub(crate) async fn safety_gates(connection: &mut PgConnection, target: MaintenanceTarget) -> Result<SafetyGates, MaintenanceError> {
    let heap = format!("public.{}", target.heap_name());
    let relation = match target {
        MaintenanceTarget::Index(index) => Some(format!("public.{}", index.relation_name())),
        MaintenanceTarget::Heap(_) => None,
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
        active_maintenance,
        waiting_relation_lock,
    })
}

/// Active foreground backend count.
fn active_backend_statement() -> SelectStatement {
    Query::select()
        .expr(Func::count(Expr::col(Asterisk)))
        .from(Alias::new("pg_stat_activity"))
        .and_where(Expr::col(Alias::new("state")).eq("active"))
        .and_where(Expr::col(Alias::new("pid")).ne(backend_pid()))
        .to_owned()
}

/// Oldest foreground transaction start.
fn oldest_transaction_statement() -> SelectStatement {
    Query::select()
        .expr(Func::min(Expr::col(Alias::new("xact_start"))))
        .from(Alias::new("pg_stat_activity"))
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

/// Whether another backend waits for a relation lock.
fn waiting_lock_statement(heap: &str, index: Option<&str>) -> SelectStatement {
    let waiting = Query::select()
        .expr(Expr::val(1))
        .from(Alias::new("pg_locks"))
        .and_where(match index {
            Some(index) => Expr::col(Alias::new("relation")).is_in([to_regclass(Expr::val(heap)), to_regclass(Expr::val(index))]),
            None => Expr::col(Alias::new("relation")).eq(to_regclass(Expr::val(heap))),
        })
        .and_where(Expr::col(Alias::new("granted")).eq(false))
        .to_owned();
    Query::select().expr(Expr::exists(waiting)).to_owned()
}
