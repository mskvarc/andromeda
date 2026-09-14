//! Schema-qualified table references for tenant context branches.
//!
//! A context branch is a PostgreSQL schema: the live branch is the connection's
//! default schema and a Snapshot branch is a dedicated one. A statement targets
//! a branch by qualifying every table it names with that branch's schema, so
//! one statement tree serves the live branch (schema absent) and any Snapshot
//! branch (schema present) with no change but the qualifier. A column reference
//! stays a bare `"table"."column"`: PostgreSQL resolves it against the range
//! table entry the qualified `FROM`/`JOIN` introduced, whatever schema that
//! entry names.
//!
//! This module holds only the qualifier mechanism. Which schema a given branch
//! maps to is decided above, in the crate that knows the domain branch
//! selector; here the schema is an opaque `sea_query` identifier.

use sea_query::{DynIden, IntoIden, TableRef};

/// Qualify `table` with the optional branch schema.
///
/// `None` yields the bare table reference resolved against the connection's
/// search path (the live branch's default schema); `Some(schema)` yields the
/// schema-qualified reference `"schema"."table"` for a Snapshot branch.
#[must_use]
pub fn qualified(schema: Option<&DynIden>, table: impl IntoIden) -> TableRef {
    match schema {
        None => TableRef::from(table.into_iden()),
        // One statement qualifies many tables with the same schema, so the
        // schema is borrowed and its `SeaRc` handle cloned per table reference.
        Some(schema) => TableRef::from((schema.clone(), table.into_iden())),
    }
}

#[cfg(test)]
mod tests {
    use crate::branch::qualified;
    use sea_query::{Alias, Asterisk, IntoIden, PostgresQueryBuilder, Query};

    fn render(schema: Option<&sea_query::DynIden>) -> String {
        let table = Alias::new("entities");
        Query::select()
            .column(Asterisk)
            .from(qualified(schema, table))
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    #[test]
    fn an_absent_schema_leaves_the_table_unqualified_for_the_live_branch() {
        assert!(render(None).contains(r#"FROM "entities""#));
    }

    #[test]
    fn a_present_schema_qualifies_the_table_for_a_snapshot_branch() {
        let schema = Alias::new("snap_7").into_iden();
        assert!(render(Some(&schema)).contains(r#"FROM "snap_7"."entities""#));
    }
}
