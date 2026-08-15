//! The statement that selects the snapshots a purge request matches
//! (clauses 5.16.7 and 6.36.3.2).

use andromeda_ngsi_ld_model::snapshot_filter::{Conjunction, SnapshotField, SnapshotOp, SnapshotPurgeFilter, SnapshotPurgeTerm, SnapshotValue};
use andromeda_postgresql_schema::snapshot::Snapshots;
use andromeda_postgresql_seaquery::{cast::cast, pg_type::PostgresType};
use sea_query::{BinOper, Condition, Expr as SeaExpr, Query, SelectStatement, SimpleExpr};

/// Build the `SELECT` that finds every snapshot a purge filter matches.
///
/// The filter is a parsed clause 4.9 subset over the members of the `Snapshot`
/// resource, and turning those members into columns is a property of this
/// schema rather than of the resource — which is where the mapping used to
/// live, on the resource's own enum in the specification model crate.
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
        SnapshotField::SnapshotPriority => Snapshots::SnapshotPriority,
        SnapshotField::Status => Snapshots::Status,
        SnapshotField::Endpoint => Snapshots::Endpoint,
        SnapshotField::CreatedAt => Snapshots::CreatedAt,
        SnapshotField::ModifiedAt => Snapshots::ModifiedAt,
        SnapshotField::ExpiresAt => Snapshots::ExpiresAt,
        SnapshotField::LastUsedAt => Snapshots::LastUsedAt,
    }
}

/// The bound operand a filter value becomes, cast where the column is not of the
/// bind's type: `status` is a PostgreSQL enum, so a text parameter compared
/// against one casts to `snapshot_status`.
fn operand(value: &SnapshotValue) -> SimpleExpr {
    match value {
        SnapshotValue::Id(id) => SeaExpr::val(id.as_str()).into(),
        SnapshotValue::Priority(priority) => SeaExpr::val(i16::from(priority.as_u8())).into(),
        SnapshotValue::Status(status) => cast(SeaExpr::val(<&str>::from(*status).to_string()).into(), PostgresType::SnapshotStatus),
        SnapshotValue::Endpoint(url) => SeaExpr::val(url.as_str()).into(),
        SnapshotValue::DateTime(instant) => SeaExpr::val(*instant).into(),
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
        assert!(
            sql_of("status==success").starts_with("SELECT \"id\" FROM \"snapshots\" WHERE \"status\" = "),
            "{}",
            sql_of("status==success")
        );
        assert!(sql_of("status==success").contains("::snapshot_status"), "{}", sql_of("status==success"));
    }

    #[test]
    fn a_conjunction_joins_its_terms_with_and() {
        let sql = sql_of("snapshotPriority>=5;status==success");
        assert!(sql.contains("\"snapshot_priority\" >= "), "{sql}");
        assert!(sql.contains(" AND \"status\" = "), "{sql}");
        assert!(sql.contains("::snapshot_status"), "{sql}");
    }

    #[test]
    fn a_disjunction_joins_its_terms_with_or() {
        let sql = sql_of("snapshotPriority<3|snapshotPriority>7");
        assert!(sql.contains("\"snapshot_priority\" < "), "{sql}");
        assert!(sql.contains(" OR \"snapshot_priority\" > "), "{sql}");
    }

    #[test]
    fn every_member_renders_the_column_the_migration_declares() {
        for (expression, column) in [
            ("id==urn:ngsi-ld:Snapshot:1", "id"),
            ("snapshotPriority==5", "snapshot_priority"),
            ("status==success", "status"),
            ("expiresAt>2024-01-01T00:00:00Z", "expires_at"),
            ("lastUsedAt<2024-01-01T00:00:00Z", "last_used_at"),
            ("createdAt>2024-01-01T00:00:00Z", "created_at"),
            ("modifiedAt>2024-01-01T00:00:00Z", "modified_at"),
        ] {
            assert!(sql_of(expression).contains(&format!("WHERE \"{column}\" ")), "{expression}");
        }
    }

    #[test]
    fn placeholders_are_contiguous_from_one() {
        let (sql, values) =
            build_match_query(&parse_snapshot_q("snapshotPriority>=5;status==success").expect("valid purge filter")).build(PostgresQueryBuilder);
        assert_eq!(values.0.len(), 2);
        assert!(sql.contains("$1"));
        assert!(sql.contains("$2"));
    }
}
