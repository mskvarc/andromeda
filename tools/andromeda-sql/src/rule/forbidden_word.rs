//! `forbidden-word`: no name carries an unregistered abbreviation or a
//! storage-type word, and a table or column name spells even a registered
//! abbreviation out.

use crate::{
    declared_schema::{column::ColumnSource, origin::Origin},
    rule::{
        Rule,
        RuleContext,
        RuleFinding,
        naming_vocabulary::{NameKind, forbidden_word, is_specification_member},
    },
};

/// A name carrying a word it may not.
#[derive(Debug, derive_more::Display)]
#[display("{name} carries the word {word}")]
struct ForbiddenWord<'schema> {
    /// The name.
    name: &'schema str,
    /// The word.
    word: &'schema str,
}

/// Every table, column, constraint, index, statistics and trigger name
/// carrying a forbidden word.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    let schema = context.schema();
    let tables = schema.tables().iter().map(|table| (table.name(), NameKind::Relation, table.origin()));
    let columns = schema
        .columns()
        .iter()
        .filter(|column| matches!(column.source(), ColumnSource::CreateTable | ColumnSource::AddColumn))
        .filter(|column| !is_specification_member(column.relation(), column.name()))
        .map(|column| (column.name(), NameKind::Relation, column.origin()));
    let constraints = schema
        .constraints()
        .iter()
        .filter_map(|constraint| Some((constraint.name()?, NameKind::Object, constraint.origin())));
    let indexes = schema.indexes().iter().map(|index| (index.name(), NameKind::Object, index.origin()));
    let statistics = schema
        .statistics()
        .iter()
        .map(|statistics| (statistics.name(), NameKind::Object, statistics.origin()));
    let triggers = schema.triggers().iter().map(|trigger| (trigger.name(), NameKind::Object, trigger.origin()));
    tables
        .chain(columns)
        .chain(constraints)
        .chain(indexes)
        .chain(statistics)
        .chain(triggers)
        .filter_map(|(name, kind, origin): (&str, NameKind, Origin)| {
            let word = forbidden_word(name, kind)?;
            Some(context.finding(Rule::ForbiddenWord, origin, name.len(), &ForbiddenWord { name, word }))
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

    const FILE: &str = "branch/0001_subscriptions.sql";

    #[test]
    fn spelled_out_names_and_specification_members_are_accepted() -> Result<(), FixtureError> {
        let sql =
            "create table subscriptions (\n    geo_q text,\n    value_geometry text,\n    constraint chk_csr_geometry check (value_geometry is not null)\n);\n";
        assert_eq!(findings(Rule::ForbiddenWord, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_storage_word_and_an_abbreviation_are_refused_at_their_names() -> Result<(), FixtureError> {
        let sql = "create table subscriptions (\n    value_geo text,\n    attr_name text\n);\n";
        assert_eq!(
            findings(Rule::ForbiddenWord, &[(FILE, sql)])?,
            ["migrations/branch/0001_subscriptions.sql:2:5", "migrations/branch/0001_subscriptions.sql:3:5"]
        );
        Ok(())
    }
}
