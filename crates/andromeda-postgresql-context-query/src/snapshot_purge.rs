//! The statement that selects the snapshots a purge request matches
//! (clauses 5.16.7 and 6.36.3.2).

use andromeda_ngsi_ld_model::snapshot_filter::{Conjunction, SnapshotField, SnapshotOp, SnapshotPurgeFilter, SnapshotValue};
use andromeda_postgresql_query::{
    bind::{BindBuilder, BindValue},
    built::BuiltQuery,
};
use std::borrow::Cow;

/// Build the `SELECT` that finds every snapshot a purge filter matches.
///
/// The filter is a parsed clause 4.9 subset over the members of the `Snapshot`
/// resource, and turning those members into columns is a property of this
/// schema rather than of the resource — which is where the mapping used to
/// live, on the resource's own enum in the specification model crate.
pub fn build_match_query(filter: &SnapshotPurgeFilter) -> BuiltQuery<'static> {
    let separator = match filter.conjunction {
        Conjunction::And => " AND ",
        Conjunction::Or => " OR ",
    };

    let mut binds = BindBuilder::default();
    let predicates: Vec<String> = filter
        .terms
        .iter()
        .map(|term| {
            let marker = binds.bind(bind_value(&term.value));
            format!("{} {} {marker}{}", column(term.field), operator(term.op), value_cast(&term.value))
        })
        .collect();

    BuiltQuery {
        sql: format!("SELECT id FROM snapshots WHERE {}", predicates.join(separator)),
        count_sql: None,
        bind_values: binds.into_values(),
    }
}

/// The `snapshots` column backing a filterable member of the resource.
const fn column(field: SnapshotField) -> &'static str {
    match field {
        SnapshotField::Id => "id",
        SnapshotField::SnapshotPriority => "snapshot_priority",
        SnapshotField::Status => "status",
        SnapshotField::Endpoint => "endpoint",
        SnapshotField::CreatedAt => "created_at",
        SnapshotField::ModifiedAt => "modified_at",
        SnapshotField::ExpiresAt => "expires_at",
        SnapshotField::LastUsedAt => "last_used_at",
    }
}

/// The SQL operator a clause 4.9 comparison renders to in this statement.
const fn operator(op: SnapshotOp) -> &'static str {
    match op {
        SnapshotOp::Eq => "=",
        SnapshotOp::Neq => "!=",
        SnapshotOp::Lt => "<",
        SnapshotOp::Lte => "<=",
        SnapshotOp::Gt => ">",
        SnapshotOp::Gte => ">=",
    }
}

/// The typed bind a filter value becomes.
fn bind_value(value: &SnapshotValue) -> BindValue<'static> {
    match value {
        SnapshotValue::Id(id) => BindValue::String(Cow::Owned(id.as_str().to_string())),
        SnapshotValue::Priority(priority) => BindValue::I16(priority.as_i16()),
        SnapshotValue::Status(status) => BindValue::String(Cow::Owned(<&str>::from(*status).to_string())),
        SnapshotValue::Endpoint(url) => BindValue::String(Cow::Owned(url.as_str().to_string())),
        SnapshotValue::DateTime(instant) => BindValue::Timestamp(*instant),
    }
}

/// The cast a bound value needs, where the column is not of the bind's type.
///
/// `status` is a PostgreSQL enum, and a text parameter compared against one has
/// to say so.
const fn value_cast(value: &SnapshotValue) -> &'static str {
    match value {
        SnapshotValue::Status(_) => "::snapshot_status",
        SnapshotValue::Id(_) | SnapshotValue::Priority(_) | SnapshotValue::Endpoint(_) | SnapshotValue::DateTime(_) => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_model::snapshot_filter::parse_snapshot_q;

    fn sql_of(raw: &str) -> String {
        build_match_query(&parse_snapshot_q(raw).expect("valid purge filter")).sql
    }

    #[test]
    fn a_single_term_renders_one_predicate_against_its_column() {
        assert_eq!(sql_of("status==success"), "SELECT id FROM snapshots WHERE status = $1::snapshot_status");
    }

    #[test]
    fn a_conjunction_joins_its_terms_with_and() {
        let sql = sql_of("snapshotPriority>=5;status==success");
        assert_eq!(sql, "SELECT id FROM snapshots WHERE snapshot_priority >= $1 AND status = $2::snapshot_status");
    }

    #[test]
    fn a_disjunction_joins_its_terms_with_or() {
        let sql = sql_of("snapshotPriority<3|snapshotPriority>7");
        assert_eq!(sql, "SELECT id FROM snapshots WHERE snapshot_priority < $1 OR snapshot_priority > $2");
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
            assert!(sql_of(expression).contains(&format!("WHERE {column} ")), "{expression}");
        }
    }

    #[test]
    fn placeholders_are_contiguous_from_one() {
        let built = build_match_query(&parse_snapshot_q("snapshotPriority>=5;status==success").expect("valid purge filter"));
        assert_eq!(built.bind_values.len(), 2);
        assert!(built.sql.contains("$1"));
        assert!(built.sql.contains("$2"));
    }
}
