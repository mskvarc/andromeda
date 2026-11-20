//! `formatter-vocabulary`: every keyword `pg_format` is taught is still used.
//!
//! `.pg_format_keywords` teaches `pg_format` the extension keywords it does not
//! know, so it cases them like its own. A word no migration writes teaches the
//! formatter nothing.

use crate::{
    formatter_vocabulary::VocabularyFile,
    rule::{Rule, RuleContext, RuleFinding},
};
use std::collections::BTreeSet;

/// A vocabulary word no migration writes.
#[derive(Debug, derive_more::Display)]
#[display("no migration uses {word}, which {file} lists")]
struct StaleWord<'input> {
    /// The word.
    word: &'input str,
    /// The file listing it.
    file: VocabularyFile,
}

/// Every vocabulary word no migration writes.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    let written = context
        .schema()
        .migrations()
        .iter()
        .flat_map(|migration| {
            migration
                .tokens()
                .iter()
                .filter(|token| !token.is_comment())
                .map(|token| migration.token_text(token).to_ascii_lowercase())
        })
        .collect::<BTreeSet<_>>();
    let keywords = context.vocabulary().keywords();
    keywords
        .entries()
        .iter()
        .filter(|entry| !written.contains(&entry.word().to_ascii_lowercase()))
        .map(|entry| {
            context.vocabulary_finding(
                Rule::FormatterVocabulary,
                keywords.file(),
                entry.range(),
                &StaleWord {
                    word: entry.word(),
                    file: keywords.file(),
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::rule::{
        Rule,
        rule_fixture::{FixtureError, findings_with, vocabulary},
    };
    use pretty_assertions::assert_eq;

    const FILE: &str = "branch/0001_values.sql";

    const SQL: &str =
        "create table values_table (\n    value_kind smallint,\n    constraint chk_values_table_presence check (num_nonnulls(value_kind) = 1)\n);\n";

    #[test]
    fn a_used_keyword_is_accepted() -> Result<(), FixtureError> {
        assert_eq!(
            findings_with(Rule::FormatterVocabulary, &[(FILE, SQL)], &vocabulary("CHECK\n"))?,
            Vec::<String>::new()
        );
        Ok(())
    }

    #[test]
    fn a_keyword_no_migration_writes_is_refused_where_it_is_listed() -> Result<(), FixtureError> {
        assert_eq!(
            findings_with(Rule::FormatterVocabulary, &[(FILE, SQL)], &vocabulary("CHECK\nBRIN\n"))?,
            [".pg_format_keywords:2:1"]
        );
        Ok(())
    }
}
