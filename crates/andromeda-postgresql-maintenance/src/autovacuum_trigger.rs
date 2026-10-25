//! The dead tuples at which autovacuum vacuums a table on its own, and the
//! modified tuples at which it analyzes it.
//!
//! Autovacuum vacuums a table once its dead tuples exceed
//! `min(threshold + scale_factor * reltuples, max_threshold)`, and analyzes it
//! once the tuples modified since its last analyze exceed
//! `threshold + scale_factor * reltuples` with the analyze parameters, which
//! have no cap. Each parameter is the table's storage parameter when the table
//! sets one and the server setting otherwise; a `max_threshold` of -1 sets no
//! cap, and a `reltuples` of -1 (never vacuumed or analyzed) counts as zero.
//! The statement computes both triggers in SQL from the table's own
//! `reloptions`, so a table the migrations tune is judged against its own
//! triggers, and reads them for every heap one branch holds.

use crate::{branch::Branch, observed_heap::WatchedHeap};
use andromeda_postgresql_seaquery::{
    cast::{cast, cast_compound},
    introspection::{current_database, current_setting, current_setting_missing_ok},
    maintenance::storage_parameter,
    operator::null_if,
    pg_type::PostgresType,
    setting::Setting,
};
use sea_query::{Alias, Expr, ExprTrait, Func, Iden, JoinType, Query, SelectStatement, SimpleExpr};

/// The alias of the statistics view the statement reads.
pub(crate) const TABLE_STATISTICS: &str = "s";

/// The alias of the `pg_class` row of each table.
const TABLE_CLASS: &str = "c";

/// The columns the trigger statement projects.
#[derive(Clone, Copy, Iden)]
pub(crate) enum TriggerColumn {
    /// The table name.
    Relname,
    /// Dead tuples past which autovacuum vacuums the table.
    VacuumTrigger,
    /// Tuples modified since the last analyze past which autovacuum analyzes
    /// the table.
    AnalyzeTrigger,
}

/// A column of the table statistics view, qualified by its alias.
pub(crate) fn statistics_column(column: &'static str) -> Expr {
    Expr::col((Alias::new(TABLE_STATISTICS), Alias::new(column)))
}

/// Whether the progress view `view` (`pg_stat_progress_vacuum`,
/// `pg_stat_progress_analyze`) reports an operation on the table of the
/// statistics row.
///
/// A progress view lists every database's operations and `relid` is an OID
/// unique only within one database, so the progress row must also name the
/// tenant's database.
pub(crate) fn progress_reported(view: &'static str) -> SimpleExpr {
    let running = Query::select()
        .expr(Expr::val(1))
        .from(Alias::new(view))
        .and_where(Expr::col((Alias::new(view), Alias::new("relid"))).equals((Alias::new(TABLE_STATISTICS), Alias::new("relid"))))
        .and_where(Expr::col((Alias::new(view), Alias::new("datname"))).eq(current_database()))
        .to_owned();
    Expr::exists(running)
}

/// The effective autovacuum vacuum and analyze triggers of every heap
/// `branch` holds that a visit watches, beside its name. Callers extend the
/// statement with the counters they compare against them.
pub(crate) fn autovacuum_triggers_statement(branch: Branch) -> SelectStatement {
    Query::select()
        .expr_as(statistics_column("relname"), TriggerColumn::Relname)
        .expr_as(vacuum_trigger(), TriggerColumn::VacuumTrigger)
        .expr_as(analyze_trigger(), TriggerColumn::AnalyzeTrigger)
        .from_as(Alias::new("pg_stat_user_tables"), Alias::new(TABLE_STATISTICS))
        .join_as(
            JoinType::InnerJoin,
            Alias::new("pg_class"),
            Alias::new(TABLE_CLASS),
            Expr::col((Alias::new(TABLE_CLASS), Alias::new("oid"))).equals((Alias::new(TABLE_STATISTICS), Alias::new("relid"))),
        )
        .and_where(statistics_column("schemaname").eq(branch.schema_name()))
        .and_where(statistics_column("relname").is_in(branch.watched_heaps().map(WatchedHeap::relation_name)))
        .to_owned()
}

/// `least(threshold + scale_factor * reltuples, max_threshold)::bigint`, each
/// parameter the table's own or the server's.
///
/// `least` ignores a NULL argument, so a cap of -1 (`nullif` makes it NULL)
/// and a server without `autovacuum_vacuum_max_threshold` (read missing-ok)
/// leave the uncapped trigger.
fn vacuum_trigger() -> SimpleExpr {
    let uncapped = uncapped_trigger(Setting::AutovacuumVacuumThreshold, Setting::AutovacuumVacuumScaleFactor);
    let cap = cast(
        null_if(
            Func::coalesce([
                table_parameter(Setting::AutovacuumVacuumMaxThreshold),
                current_setting_missing_ok(Setting::AutovacuumVacuumMaxThreshold),
            ])
            .into(),
            Expr::val("-1"),
        ),
        PostgresType::Numeric,
    );
    cast(Func::least([uncapped, cap]).into(), PostgresType::Bigint)
}

/// `(threshold + scale_factor * reltuples)::bigint` with the analyze
/// parameters, each the table's own or the server's.
fn analyze_trigger() -> SimpleExpr {
    cast(
        uncapped_trigger(Setting::AutovacuumAnalyzeThreshold, Setting::AutovacuumAnalyzeScaleFactor),
        PostgresType::Bigint,
    )
}

/// `threshold + scale_factor * reltuples` in `numeric`, the two named
/// parameters each the table's own or the server's.
fn uncapped_trigger(threshold: Setting, scale_factor: Setting) -> SimpleExpr {
    let reltuples: SimpleExpr = Func::greatest([
        cast(Expr::col((Alias::new(TABLE_CLASS), Alias::new("reltuples"))), PostgresType::Numeric),
        Expr::val(0),
    ])
    .into();
    let threshold = cast(effective_parameter(threshold), PostgresType::Numeric);
    let scale_factor = cast(effective_parameter(scale_factor), PostgresType::Numeric);
    cast_compound(threshold.add(scale_factor.mul(reltuples)), PostgresType::Numeric)
}

/// A storage parameter as the table sets it, falling back to the server
/// setting of the same name.
fn effective_parameter(name: Setting) -> SimpleExpr {
    Func::coalesce([table_parameter(name), current_setting(name)]).into()
}

/// A storage parameter as the table sets it, NULL when it does not.
fn table_parameter(name: Setting) -> SimpleExpr {
    storage_parameter(Expr::col((Alias::new(TABLE_CLASS), Alias::new("reloptions"))), name)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{autovacuum_trigger::autovacuum_triggers_statement, branch::Branch, candidate::heap_target::HeapTarget, observed_heap::ObservedHeap};
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use andromeda_postgresql_seaquery::setting::Setting;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn trigger_statement_reads_reloptions_with_server_fallback() {
        let sql = autovacuum_triggers_statement(Branch::Live).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"pg_options_to_table("c"."reloptions")"#), "{sql}");
        for parameter in [
            Setting::AutovacuumVacuumThreshold,
            Setting::AutovacuumVacuumScaleFactor,
            Setting::AutovacuumAnalyzeThreshold,
            Setting::AutovacuumAnalyzeScaleFactor,
        ] {
            assert!(sql.contains(&format!("\"option_name\" = '{parameter}'")), "{parameter}: {sql}");
            assert!(sql.contains(&format!("current_setting('{parameter}')")), "{parameter}: {sql}");
        }
        assert!(sql.contains(r#""option_name" = 'autovacuum_vacuum_max_threshold'"#), "{sql}");
        assert!(sql.contains("current_setting('autovacuum_vacuum_max_threshold', TRUE)"), "{sql}");
        assert!(!sql.contains("autovacuum_analyze_max_threshold"), "analyze has no cap: {sql}");
        assert!(sql.contains("'-1')"), "a cap of -1 sets none: {sql}");
        assert!(sql.contains(r#"GREATEST("c"."reltuples"::numeric, 0)"#), "{sql}");
        assert!(sql.contains(r#"AS "vacuum_trigger""#), "{sql}");
        assert!(sql.contains(r#"AS "analyze_trigger""#), "{sql}");
        assert!(sql.contains(r#"JOIN "pg_class" AS "c" ON "c"."oid" = "s"."relid""#), "{sql}");
        assert!(sql.contains(r#""s"."schemaname" = 'public'"#), "{sql}");
        for heap in HeapTarget::ALL {
            assert!(sql.contains(&format!("'{}'", heap.relation_name())), "{heap}: {sql}");
        }
        for heap in ObservedHeap::ALL {
            assert!(sql.contains(&format!("'{}'", heap.relation_name())), "{heap:?}: {sql}");
        }
    }

    #[test]
    fn a_snapshot_branch_trigger_reads_only_its_branch_heaps() {
        let branch = Branch::Snapshot(SnapshotKey::try_from(3).expect("positive snapshot key"));
        let sql = autovacuum_triggers_statement(branch).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""s"."schemaname" = 'snap_3'"#), "{sql}");
        for heap in HeapTarget::ALL {
            let named = sql.contains(&format!("'{}'", heap.relation_name()));
            assert_eq!(named, branch.heaps().any(|held| held == heap), "{heap}: {sql}");
        }
    }
}
