//! `c-collation`: every text value the migrations store is stored, matched
//! and compared under the `C` collation, and no other collation is named.
//!
//! Every column, composite type field and domain of a collatable type (`text`
//! and its kin, and arrays of them) declares `collate "C"`, a retyping to one
//! included, since `alter column ... type text` without it resets the column
//! to the database default. No `collate` anywhere in a migration names
//! another collation, so no index element overrides a column's `C`. String
//! equality is code-unit comparison (ETSI GS CIM 009 v1.9.1 clause 4.9, IETF
//! RFC 8259 section 8.3) and temporal `min` and `max` are lexicographic, both
//! byte order in UTF-8; a request's ordering names its ICU collation in the
//! query itself, so no stored value needs another.

use crate::{
    declared_schema::parsed_migration::ParsedMigration,
    rule::{Rule, RuleContext, RuleFinding},
};
use pg_query::protobuf::Token;

/// The one collation a migration names.
const C_COLLATION: &str = "C";

/// What the rule finds.
#[derive(Debug, derive_more::Display)]
enum CollationViolation<'schema> {
    /// A collatable column or field names no collation.
    #[display("{relation}.{column} is {data_type} without collate \"C\"")]
    UncollatedColumn {
        /// The table or composite type.
        relation: &'schema str,
        /// The column.
        column: &'schema str,
        /// Its type, as written.
        data_type: String,
    },
    /// A domain of a collatable type names no collation.
    #[display("the domain {domain} is {data_type} without collate \"C\"")]
    UncollatedDomain {
        /// The domain.
        domain: &'schema str,
        /// Its base type, as written.
        data_type: String,
    },
    /// A collation other than `C` is named.
    #[display("collate names {_0}, where every text value is collated \"C\"")]
    OtherCollation(String),
}

/// Every collatable column, field and domain without a collation, and every
/// collation named other than `C`.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    let schema = context.schema();
    let columns = schema
        .columns()
        .iter()
        .filter(|column| column.collation().is_none() && schema.is_collatable(column.data_type()))
        .map(|column| {
            context.finding(
                Rule::CCollation,
                column.origin(),
                column.name().len(),
                &CollationViolation::UncollatedColumn {
                    relation: column.relation(),
                    column: column.name(),
                    data_type: column.data_type().written(),
                },
            )
        });
    let domains = schema
        .domains()
        .iter()
        .filter(|domain| domain.collation().is_none() && schema.is_collatable(domain.base()))
        .map(|domain| {
            context.finding(
                Rule::CCollation,
                domain.origin(),
                domain.name().len(),
                &CollationViolation::UncollatedDomain {
                    domain: domain.name(),
                    data_type: domain.base().written(),
                },
            )
        });
    let named = schema.migrations().iter().flat_map(|migration| {
        other_collations(migration)
            .into_iter()
            .map(move |(offset, length, name)| context.finding(Rule::CCollation, migration.origin(offset), length, &CollationViolation::OtherCollation(name)))
    });
    columns.chain(domains).chain(named).collect()
}

/// Every collation `migration` names after `collate` other than `C`: the
/// offset and length of its last name part, and the name as written.
fn other_collations(migration: &ParsedMigration) -> Vec<(usize, usize, String)> {
    let code = migration.tokens().iter().filter(|token| !token.is_comment()).collect::<Vec<_>>();
    code.iter()
        .enumerate()
        .filter(|(_, token)| token.kind() == Token::Collate)
        .filter_map(|(position, _)| {
            let mut last = code.get(position + 1)?;
            let mut next = position + 2;
            while code.get(next).is_some_and(|dot| dot.kind() == Token::Ascii46) {
                last = code.get(next + 1)?;
                next += 2;
            }
            Some(*last)
        })
        .filter_map(|name| {
            let written = migration.token_text(name);
            let collation = written
                .strip_prefix('"')
                .and_then(|quoted| quoted.strip_suffix('"'))
                .map_or_else(|| written.to_ascii_lowercase(), str::to_owned);
            (collation != C_COLLATION).then(|| (name.range().start, name.range().len(), written.to_owned()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::rule::{
        Rule,
        rule_fixture::{FixtureError, findings},
    };
    use pretty_assertions::assert_eq;

    const FILE: &str = "tenant/0001_tenants.sql";

    #[test]
    fn a_primary_key_over_c_text_is_accepted() -> Result<(), FixtureError> {
        let sql = "create table tenants (\n    name text compression lz4 collate \"C\" not null,\n    constraint pk_tenants primary key (name)\n);\n";
        assert_eq!(findings(Rule::CCollation, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_primary_key_over_text_without_c_is_refused_at_the_column() -> Result<(), FixtureError> {
        let sql = "create table tenants (\n    name text compression lz4 not null,\n    constraint pk_tenants primary key (name)\n);\n";
        assert_eq!(findings(Rule::CCollation, &[(FILE, sql)])?, ["migrations/tenant/0001_tenants.sql:2:5"]);
        Ok(())
    }

    #[test]
    fn a_unique_constraint_over_c_text_is_accepted() -> Result<(), FixtureError> {
        let sql = "create table tenants (\n    tenant_key bigint not null,\n    database_name text collate \"C\",\n    constraint uq_tenants_database_name unique (database_name)\n);\n";
        assert_eq!(findings(Rule::CCollation, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_unique_constraint_over_text_without_c_is_refused_at_the_column() -> Result<(), FixtureError> {
        let sql = "create table tenants (\n    tenant_key bigint not null,\n    database_name text,\n    constraint uq_tenants_database_name unique (database_name)\n);\n";
        assert_eq!(findings(Rule::CCollation, &[(FILE, sql)])?, ["migrations/tenant/0001_tenants.sql:3:5"]);
        Ok(())
    }

    #[test]
    fn a_partial_index_over_c_text_is_accepted() -> Result<(), FixtureError> {
        let sql = "create table tenants (\n    name text collate \"C\"\n);\n\ncreate index idx_tenants_name on tenants (name collate \"C\")\nwhere\n    name is not null;\n";
        assert_eq!(findings(Rule::CCollation, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_partial_index_overriding_c_is_refused_at_the_collation() -> Result<(), FixtureError> {
        let sql = "create table tenants (\n    name text collate \"C\"\n);\n\ncreate index idx_tenants_name on tenants (name collate \"und-x-icu\")\nwhere\n    name is not null;\n";
        assert_eq!(findings(Rule::CCollation, &[(FILE, sql)])?, ["migrations/tenant/0001_tenants.sql:5:56"]);
        Ok(())
    }

    #[test]
    fn a_retyping_to_text_without_c_is_refused() -> Result<(), FixtureError> {
        let sql = "create table tenants (\n    name text collate \"C\"\n);\n\nalter table tenants\n    alter column name type text;\n";
        assert_eq!(findings(Rule::CCollation, &[(FILE, sql)])?, ["migrations/tenant/0001_tenants.sql:6:18"]);
        Ok(())
    }

    #[test]
    fn a_composite_field_and_a_domain_of_text_need_c() -> Result<(), FixtureError> {
        let sql = "create type label as (\n    text_value text,\n    labels text[] collate \"C\"\n);\n\ncreate domain short_text as text;\n";
        assert_eq!(
            findings(Rule::CCollation, &[(FILE, sql)])?,
            ["migrations/tenant/0001_tenants.sql:2:5", "migrations/tenant/0001_tenants.sql:6:15"]
        );
        Ok(())
    }

    #[test]
    fn the_rule_does_not_fire_on_a_column_of_another_type() -> Result<(), FixtureError> {
        let sql = "create table tenants (\n    tenant_key bigint not null,\n    payload jsonb compression lz4\n);\n";
        assert_eq!(findings(Rule::CCollation, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }
}
