//! The statement that selects the snapshots a purge request matches
//! (clauses 5.16.7 and 6.36.3.2).

use andromeda_ngsi_ld_model::snapshot_filter::{Conjunction, SnapshotField, SnapshotOp, SnapshotPurgeFilter, SnapshotPurgeTerm, SnapshotValue};
use andromeda_postgresql_schema::snapshot::Snapshots;
use sea_query::{BinOper, Condition, Expr as SeaExpr, ExprTrait, Query, SelectStatement, SimpleExpr};

/// Build the `SELECT` that finds every snapshot a purge filter matches.
///
/// The filter is a parsed clause 4.9 subset over the members of the `Snapshot`
/// resource, and turning those members into columns is a property of this
/// schema rather than of the resource — which is where the mapping used to
/// live, on the resource's own enum in the specification model crate.
#[must_use]
pub fn build_match_query(filter: &SnapshotPurgeFilter) -> SelectStatement {
    let mut condition = match filter.conjunction {
        Conjunction::And => Condition::all(),
        Conjunction::Or => Condition::any(),
    };
    for term in &filter.terms {
        condition = condition.add(term_predicate(term));
    }

    let mut select = Query::select();
    select.column(Snapshots::Id).from(Snapshots::Table).cond_where(condition);
    select
}

/// One filter term compiled to its column comparison.
fn term_predicate(term: &SnapshotPurgeTerm) -> SimpleExpr {
    if let SnapshotValue::Type(resource_type) = &term.value {
        let type_matches = resource_type.as_str() == "Snapshot";
        let predicate_matches = match term.op {
            SnapshotOp::Eq => type_matches,
            SnapshotOp::Neq => !type_matches,
            SnapshotOp::Lt | SnapshotOp::Lte | SnapshotOp::Gt | SnapshotOp::Gte => false,
        };
        return SeaExpr::val(predicate_matches);
    }

    let column = SeaExpr::expr(SeaExpr::col(snapshot_column(term.field)));
    let operand = operand(&term.value);
    match term.op {
        SnapshotOp::Eq => column.eq(operand),
        // `!=` is kept over the equivalent `<>` so the statement reads as it did.
        SnapshotOp::Neq => column.binary(BinOper::Custom("!="), operand),
        SnapshotOp::Lt => column.lt(operand),
        SnapshotOp::Lte => column.lte(operand),
        SnapshotOp::Gt => column.gt(operand),
        SnapshotOp::Gte => column.gte(operand),
    }
}

/// The `snapshots` column backing a filterable member of the resource.
const fn snapshot_column(field: SnapshotField) -> Snapshots {
    match field {
        SnapshotField::Id => Snapshots::Id,
        SnapshotField::Type => Snapshots::Id,
        SnapshotField::SnapshotPriority => Snapshots::SnapshotPriority,
        SnapshotField::Endpoint => Snapshots::Endpoint,
    }
}

/// The bound operand a filter value becomes.
fn operand(value: &SnapshotValue) -> SimpleExpr {
    match value {
        SnapshotValue::Id(id) => SeaExpr::val(id.as_str()),
        SnapshotValue::Type(resource_type) => SeaExpr::val(resource_type.as_str() == "Snapshot"),
        SnapshotValue::Priority(priority) => SeaExpr::val(i16::from(priority.as_u8())),
        SnapshotValue::Endpoint(url) => SeaExpr::val(url.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_model::snapshot_filter::parse_snapshot_q;
    use sea_query::PostgresQueryBuilder;

    fn sql_of(raw: &str) -> String {
        build_match_query(&parse_snapshot_q(raw).expect("valid purge filter")).to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_single_term_renders_one_predicate_against_its_column() {
        let sql = sql_of("id==urn:ngsi-ld:Snapshot:1");
        assert!(sql.starts_with("SELECT \"id\" FROM \"snapshots\" WHERE \"id\" = "), "{sql}");
    }

    #[test]
    fn a_conjunction_joins_its_terms_with_and() {
        let sql = sql_of("snapshotPriority==5;endpoint==https://example.org/status");
        assert!(sql.contains("\"snapshot_priority\" = "), "{sql}");
        assert!(sql.contains(" AND \"endpoint\" = "), "{sql}");
    }

    #[test]
    fn a_disjunction_joins_its_terms_with_or() {
        let sql = sql_of("id==urn:ngsi-ld:Snapshot:1|snapshotPriority!=7");
        assert!(sql.contains("\"id\" = "), "{sql}");
        assert!(sql.contains(" OR (\"snapshot_priority\" != "), "{sql}");
    }

    #[test]
    fn every_member_renders_the_column_the_migration_declares() {
        for (expression, column) in [
            ("id==urn:ngsi-ld:Snapshot:1", "id"),
            ("snapshotPriority==5", "snapshot_priority"),
            ("endpoint==https://example.org/status", "endpoint"),
        ] {
            assert!(sql_of(expression).contains(&format!("WHERE \"{column}\" ")), "{expression}");
        }
        assert!(sql_of("type==Snapshot").ends_with("WHERE TRUE"));
        assert!(sql_of("type!=Snapshot").ends_with("WHERE FALSE"));
    }

    #[test]
    fn placeholders_are_contiguous_from_one() {
        let (sql, values) = build_match_query(&parse_snapshot_q("snapshotPriority==5;endpoint==https://example.org/status").expect("valid purge filter"))
            .build(PostgresQueryBuilder);
        assert_eq!(values.0.len(), 2);
        assert!(sql.contains("$1"));
        assert!(sql.contains("$2"));
    }
}
