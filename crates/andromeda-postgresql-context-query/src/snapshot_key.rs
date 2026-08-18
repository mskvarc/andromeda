//! The scalar subquery that resolves a snapshot's surrogate key from its URI.
//!
//! `snapshots.snapshot_key` is a database-issued surrogate the two child
//! tables reference; the API only ever names a snapshot by its URI (clause
//! 5.2.22 `id`). Every statement that starts from the URI and reaches a child
//! table therefore resolves the surrogate with the one-row subquery built here,
//! rather than fetching the key in a separate round trip: the subquery is a
//! probe of the unique index on `snapshots.id`, and its result plans as a
//! constant for the child-table scan it feeds.

use andromeda_postgresql_schema::snapshot::Snapshots;
use sea_query::{Expr, ExprTrait, Query, QueryStatementBuilder, SimpleExpr};

/// `(SELECT snapshots.snapshot_key FROM snapshots WHERE snapshots.id = <id>)`.
///
/// `id` is the expression the URI arrives as - a bound value, an external
/// placeholder or a column reference - so the same subquery serves a bind-once
/// statement and a fully bound one alike. Both references are table-qualified
/// so an enclosing statement that also exposes an `id` cannot capture them.
pub fn snapshot_key_of(id: SimpleExpr) -> SimpleExpr {
    let mut lookup = Query::select();
    lookup
        .column((Snapshots::Table, Snapshots::SnapshotKey))
        .from(Snapshots::Table)
        .and_where(Expr::col((Snapshots::Table, Snapshots::Id)).eq(id));
    SimpleExpr::SubQuery(None, Box::new(lookup.into_sub_query_statement()))
}

#[cfg(test)]
mod tests {
    use crate::snapshot_key::snapshot_key_of;
    use andromeda_postgresql_seaquery::placeholder::external_placeholder;
    use sea_query::{Alias, Expr, ExprTrait, PostgresQueryBuilder, Query};

    #[test]
    fn the_key_lookup_is_a_qualified_scalar_subquery_on_the_unique_identifier() {
        let sql = Query::select()
            .expr(Expr::col(Alias::new("entity_data")))
            .from(Alias::new("snapshot_entities"))
            .and_where(Expr::col(Alias::new("snapshot_key")).eq(snapshot_key_of(external_placeholder(1))))
            .to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#""snapshot_key" = (SELECT "snapshots"."snapshot_key" FROM "snapshots" WHERE "snapshots"."id" = ($1))"#),
            "{sql}"
        );
    }

    #[test]
    fn a_bound_identifier_binds_inside_the_subquery() {
        let (sql, values) = Query::select()
            .expr(Expr::col(Alias::new("entity_data")))
            .from(Alias::new("snapshot_entities"))
            .and_where(Expr::col(Alias::new("snapshot_key")).eq(snapshot_key_of(Expr::val("urn:ngsi-ld:Snapshot:1"))))
            .build(PostgresQueryBuilder);
        assert!(sql.contains(r#""snapshots"."id" = $1"#), "{sql}");
        assert_eq!(values.0.len(), 1);
    }
}
