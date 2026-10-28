//! `lz4-compression`: every column of a type PostgreSQL can compress declares
//! `compression lz4`, in `create table` and in `add column`, or is given it by
//! an `alter column ... set compression lz4` of the same migration. Squawk
//! cannot parse `compression` inside `add column`, so an added column takes
//! the method from a separate `set compression`.
//!
//! A type is compressible when its values are stored with a length header:
//! every variable-length built-in type, every array and every composite type,
//! and every domain over one of them. The clause only chooses the algorithm
//! (PostgreSQL compresses a value once its row passes about 2 kB), and naming
//! it on the column keeps the choice out of the server's
//! `default_toast_compression`.

use crate::{
    declared_schema::{
        column::{ColumnSource, CompressionSetting, DeclaredColumn},
        declared_type::Storage,
    },
    rule::{Rule, RuleContext, RuleFinding},
};

/// The compression method every compressible column declares.
const LZ4: &str = "lz4";

/// A compressible column without `compression lz4`.
#[derive(Debug, derive_more::Display)]
#[display("{relation}.{column} is {data_type}, which PostgreSQL compresses, and declares no compression lz4")]
struct UncompressedColumn<'schema> {
    /// The table.
    relation: &'schema str,
    /// The column.
    column: &'schema str,
    /// Its type, as written.
    data_type: String,
}

/// Every table column of a compressible type that declares another method or
/// none and is not set to lz4 later in its migration.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    let schema = context.schema();
    schema
        .columns()
        .iter()
        .filter(|column| matches!(column.source(), ColumnSource::CreateTable | ColumnSource::AddColumn))
        .filter(|column| schema.storage(column.data_type()) == Some(Storage::VariableLength))
        .filter(|column| column.compression() != Some(LZ4))
        .filter(|column| !is_set_to_lz4(schema.compression_settings(), column))
        .map(|column| {
            context.finding(
                Rule::Lz4Compression,
                column.origin(),
                column.name().len(),
                &UncompressedColumn {
                    relation: column.relation(),
                    column: column.name(),
                    data_type: column.data_type().written(),
                },
            )
        })
        .collect()
}

/// Whether a `set compression lz4` after `column`'s declaration, in the same
/// migration, gives it lz4.
fn is_set_to_lz4(settings: &[CompressionSetting], column: &DeclaredColumn) -> bool {
    settings.iter().any(|setting| {
        setting.relation() == column.relation()
            && setting.column() == column.name()
            && setting.method() == LZ4
            && setting.origin().version() == column.origin().version()
            && setting.origin().offset() > column.origin().offset()
    })
}

#[cfg(test)]
mod tests {
    use crate::rule::{
        Rule,
        rule_fixture::{FixtureError, findings},
    };
    use pretty_assertions::assert_eq;

    const FILE: &str = "tenant/0001_entities.sql";

    #[test]
    fn a_text_column_with_lz4_in_create_table_is_accepted() -> Result<(), FixtureError> {
        assert_eq!(
            findings(
                Rule::Lz4Compression,
                &[(FILE, "create table entities (\n    id text compression lz4 not null\n);\n")]
            )?,
            Vec::<String>::new()
        );
        Ok(())
    }

    #[test]
    fn a_text_column_without_lz4_in_create_table_is_refused_at_its_name() -> Result<(), FixtureError> {
        assert_eq!(
            findings(Rule::Lz4Compression, &[(FILE, "create table entities (\n    id text not null\n);\n")])?,
            ["migrations/tenant/0001_entities.sql:2:5"]
        );
        Ok(())
    }

    #[test]
    fn a_text_column_with_lz4_in_add_column_is_accepted() -> Result<(), FixtureError> {
        let sql = "create table entities (\n    entity_key bigint not null\n);\n\nalter table entities\n    add column note text compression lz4;\n";
        assert_eq!(findings(Rule::Lz4Compression, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_text_column_without_lz4_in_add_column_is_refused_at_its_name() -> Result<(), FixtureError> {
        let sql = "create table entities (\n    entity_key bigint not null\n);\n\nalter table entities\n    add column note text;\n";
        assert_eq!(findings(Rule::Lz4Compression, &[(FILE, sql)])?, ["migrations/tenant/0001_entities.sql:6:16"]);
        Ok(())
    }

    #[test]
    fn an_added_column_set_to_lz4_later_in_its_migration_is_accepted() -> Result<(), FixtureError> {
        let sql = "create table entities (\n    entity_key bigint not null\n);\n\nalter table entities\n    add column note text;\n\nalter table entities\n    alter column note set compression lz4;\n";
        assert_eq!(findings(Rule::Lz4Compression, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn an_added_column_set_to_lz4_in_a_later_migration_is_refused() -> Result<(), FixtureError> {
        let first = "create table entities (\n    entity_key bigint not null\n);\n\nalter table entities\n    add column note text;\n";
        let second = "alter table entities\n    alter column note set compression lz4;\n";
        assert_eq!(
            findings(Rule::Lz4Compression, &[(FILE, first), ("tenant/0002_notes.sql", second)])?,
            ["migrations/tenant/0001_entities.sql:6:16"]
        );
        Ok(())
    }

    #[test]
    fn an_added_column_set_to_pglz_is_refused() -> Result<(), FixtureError> {
        let sql = "create table entities (\n    entity_key bigint not null\n);\n\nalter table entities\n    add column note text;\n\nalter table entities\n    alter column note set compression pglz;\n";
        assert_eq!(findings(Rule::Lz4Compression, &[(FILE, sql)])?, ["migrations/tenant/0001_entities.sql:6:16"]);
        Ok(())
    }

    #[test]
    fn a_bit_string_an_array_and_a_composite_are_compressible() -> Result<(), FixtureError> {
        let sql = "create type pair as (\n    left_value integer\n);\n\ncreate table t (\n    mask bit(80) not null,\n    keys bigint[],\n    pair pair\n);\n";
        assert_eq!(
            findings(Rule::Lz4Compression, &[(FILE, sql)])?,
            [
                "migrations/tenant/0001_entities.sql:6:5",
                "migrations/tenant/0001_entities.sql:7:5",
                "migrations/tenant/0001_entities.sql:8:5"
            ]
        );
        Ok(())
    }

    #[test]
    fn the_rule_does_not_fire_on_a_fixed_width_column() -> Result<(), FixtureError> {
        let sql = "create table t (\n    entity_key bigint not null,\n    seen_at timestamptz,\n    is_live boolean\n);\n";
        assert_eq!(findings(Rule::Lz4Compression, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_composite_type_field_takes_no_compression_clause() -> Result<(), FixtureError> {
        assert_eq!(
            findings(Rule::Lz4Compression, &[(FILE, "create type pair as (\n    label text\n);\n")])?,
            Vec::<String>::new()
        );
        Ok(())
    }
}
