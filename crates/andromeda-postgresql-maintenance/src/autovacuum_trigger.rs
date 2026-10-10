//! The dead tuples at which autovacuum vacuums a table on its own.
//!
//! Autovacuum vacuums a table once its dead tuples exceed
//! `min(threshold + scale_factor * reltuples, max_threshold)`. Each of the
//! three parameters is the table's storage parameter when the table sets one
//! and the server setting otherwise; a `max_threshold` of -1 sets no cap, and
//! a `reltuples` of -1 (never vacuumed or analyzed) counts as zero. The
//! statement computes the trigger in SQL from the table's own `reloptions`, so
//! a table the migrations tune is judged against its own trigger, and reads it
//! for every heap one branch holds.

use crate::{branch::Branch, candidate::HeapTarget};
use andromeda_postgresql_seaquery::{
    cast::{cast, cast_compound},
    introspection::{current_setting, current_setting_missing_ok},
    maintenance::storage_parameter,
    operator::null_if,
    pg_type::PostgresType,
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
}

/// A column of the table statistics view, qualified by its alias.
pub(crate) fn statistics_column(column: &'static str) -> Expr {
    Expr::col((Alias::new(TABLE_STATISTICS), Alias::new(column)))
}

/// The effective autovacuum vacuum trigger of every heap `branch` holds,
/// beside its name. Callers extend the statement with the counters they
/// compare against it.
pub(crate) fn autovacuum_triggers_statement(branch: Branch) -> SelectStatement {
    Query::select()
        .expr_as(statistics_column("relname"), TriggerColumn::Relname)
        .expr_as(vacuum_trigger(), TriggerColumn::VacuumTrigger)
        .from_as(Alias::new("pg_stat_user_tables"), Alias::new(TABLE_STATISTICS))
        .join_as(
            JoinType::InnerJoin,
            Alias::new("pg_class"),
            Alias::new(TABLE_CLASS),
            Expr::col((Alias::new(TABLE_CLASS), Alias::new("oid"))).equals((Alias::new(TABLE_STATISTICS), Alias::new("relid"))),
        )
        .and_where(statistics_column("schemaname").eq(branch.schema_name()))
        .and_where(statistics_column("relname").is_in(branch.heaps().map(HeapTarget::relation_name)))
        .to_owned()
}

/// `least(threshold + scale_factor * reltuples, max_threshold)::bigint`, each
/// parameter the table's own or the server's.
///
/// `least` ignores a NULL argument, so a cap of -1 (`nullif` makes it NULL)
/// and a server without `autovacuum_vacuum_max_threshold` (read missing-ok)
/// leave the uncapped trigger.
fn vacuum_trigger() -> SimpleExpr {
    let reltuples: SimpleExpr = Func::greatest([
        cast(Expr::col((Alias::new(TABLE_CLASS), Alias::new("reltuples"))), PostgresType::Numeric),
        Expr::val(0),
    ])
    .into();
    let threshold = cast(effective_parameter("autovacuum_vacuum_threshold"), PostgresType::Numeric);
    let scale_factor = cast(effective_parameter("autovacuum_vacuum_scale_factor"), PostgresType::Numeric);
    let cap = cast(
        null_if(
            Func::coalesce([
                table_parameter("autovacuum_vacuum_max_threshold"),
                current_setting_missing_ok("autovacuum_vacuum_max_threshold"),
            ])
            .into(),
            Expr::val("-1"),
        ),
        PostgresType::Numeric,
    );
    let uncapped = cast_compound(threshold.add(scale_factor.mul(reltuples)), PostgresType::Numeric);
    cast(Func::least([uncapped, cap]).into(), PostgresType::Bigint)
}

/// A storage parameter as the table sets it, falling back to the server
/// setting of the same name.
fn effective_parameter(name: &'static str) -> SimpleExpr {
    Func::coalesce([table_parameter(name), current_setting(name)]).into()
}

/// A storage parameter as the table sets it, NULL when it does not.
fn table_parameter(name: &'static str) -> SimpleExpr {
    storage_parameter(Expr::col((Alias::new(TABLE_CLASS), Alias::new("reloptions"))), name)
}

#[cfg(test)]
mod tests {
    use crate::{autovacuum_trigger::autovacuum_triggers_statement, branch::Branch, candidate::HeapTarget};
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn trigger_statement_reads_reloptions_with_server_fallback() {
        let sql = autovacuum_triggers_statement(Branch::Live).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"pg_options_to_table("c"."reloptions")"#), "{sql}");
        for parameter in ["autovacuum_vacuum_threshold", "autovacuum_vacuum_scale_factor"] {
            assert!(sql.contains(&format!("\"option_name\" = '{parameter}'")), "{parameter}: {sql}");
            assert!(sql.contains(&format!("current_setting('{parameter}')")), "{parameter}: {sql}");
        }
        assert!(sql.contains(r#""option_name" = 'autovacuum_vacuum_max_threshold'"#), "{sql}");
        assert!(sql.contains("current_setting('autovacuum_vacuum_max_threshold', TRUE)"), "{sql}");
        assert!(sql.contains("'-1')"), "a cap of -1 sets none: {sql}");
        assert!(sql.contains(r#"GREATEST("c"."reltuples"::numeric, 0)"#), "{sql}");
        assert!(sql.contains(r#"JOIN "pg_class" AS "c" ON "c"."oid" = "s"."relid""#), "{sql}");
        assert!(sql.contains(r#""s"."schemaname" = 'public'"#), "{sql}");
        for heap in HeapTarget::ALL {
            assert!(sql.contains(&format!("'{}'", heap.relation_name())), "{heap}: {sql}");
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
