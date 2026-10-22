//! Quoting names into the text of a command sea-query has no form for.
//!
//! A command names relations, schemas, databases and columns as identifiers,
//! because PostgreSQL takes none as a bound parameter in a utility statement.
//! They are quoted by the same builder that renders every sea-query statement,
//! so an embedded quote is escaped rather than closing the name early. A
//! literal a command carries is quoted by [`crate::literal`].

use sea_query::{Alias, DynIden, Iden, IntoIden, PostgresQueryBuilder, QuotedBuilder, TableName, TableRefBuilder};

/// `iden` as a quoted identifier: `"name"`.
pub(crate) fn quoted_iden(iden: &DynIden) -> String {
    let mut sql = String::new();
    PostgresQueryBuilder.prepare_iden(iden, &mut sql);
    sql
}

/// The identifier `name` stands for, quoted.
pub(crate) fn quoted_name(name: &dyn Iden) -> String {
    quoted_iden(&Alias::new(name.to_string()).into_iden())
}

/// `relation.column` as a quoted, qualified column reference:
/// `"relation"."column"`.
pub(crate) fn quoted_column(relation: &dyn Iden, column: &dyn Iden) -> String {
    format!("{}.{}", quoted_name(relation), quoted_name(column))
}

/// `table` as a quoted, possibly schema-qualified relation name:
/// `"schema"."table"`.
pub(crate) fn quoted_table(table: &TableName) -> String {
    let mut sql = String::new();
    PostgresQueryBuilder.prepare_table_name(table, &mut sql);
    sql
}

/// The quoted relation names of `tables`, comma-separated.
pub(crate) fn quoted_tables(tables: &[TableName]) -> String {
    tables.iter().map(quoted_table).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use crate::identifier::{quoted_name, quoted_table};
    use sea_query::{Alias, TableName};

    #[test]
    fn an_embedded_quote_is_doubled_inside_an_identifier() {
        pretty_assertions::assert_eq!(quoted_name(&Alias::new("a\"b")), r#""a""b""#);
    }

    #[test]
    fn a_qualified_relation_quotes_its_schema_and_its_name() {
        let table = TableName::from((Alias::new("snap_7"), Alias::new("entities")));
        pretty_assertions::assert_eq!(quoted_table(&table), r#""snap_7"."entities""#);
    }
}
