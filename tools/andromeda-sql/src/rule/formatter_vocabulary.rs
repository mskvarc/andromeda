//! `formatter-vocabulary`: `pg_format` knows every function a migration calls,
//! and every word it is taught is still used.
//!
//! `pg_format` writes a call of a function it knows as `name(` and of one it
//! does not as `name (`, so a function it does not know, a trigger's `execute
//! function` target included, is laid out unlike every other call until
//! `.pg_format_functions` lists it. Which functions it knows is asked of the
//! pinned `pg_format` once per run. A word in `.pg_format_functions` that no
//! migration calls, or in `.pg_format_keywords` that no migration writes, is
//! teaching the formatter nothing.

use crate::{
    formatter_vocabulary::{VocabularyFile, VocabularyList},
    rule::{Rule, RuleContext, RuleFinding},
};
use std::collections::BTreeSet;

/// What the rule finds.
#[derive(Debug, derive_more::Display)]
enum VocabularyViolation<'input> {
    /// A called function the formatter does not know.
    #[display("pg_format does not know the function {_0}, so it lays the call out as `{_0} (`")]
    UnknownFunction(&'input str),
    /// A vocabulary word no migration uses.
    #[display("no migration uses {word}, which {file} lists")]
    StaleWord {
        /// The word.
        word: &'input str,
        /// The file listing it.
        file: VocabularyFile,
    },
}

/// Every call of a function `pg_format` does not know, and every vocabulary
/// word no migration uses.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    let schema = context.schema();
    let vocabulary = context.vocabulary();
    let called = schema
        .calls()
        .iter()
        .map(|call| call.name().name().to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let written = schema
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
    let unknown = schema.calls().iter().filter(|call| !vocabulary.knows(call.name().name())).map(|call| {
        context.finding(
            Rule::FormatterVocabulary,
            call.origin(),
            call.name().name().len(),
            &VocabularyViolation::UnknownFunction(call.name().name()),
        )
    });
    let stale = |list: &'_ VocabularyList, used: &BTreeSet<String>| {
        list.entries()
            .iter()
            .filter(|entry| !used.contains(&entry.word().to_ascii_lowercase()))
            .map(|entry| {
                context.vocabulary_finding(
                    Rule::FormatterVocabulary,
                    list.file(),
                    entry.range(),
                    &VocabularyViolation::StaleWord {
                        word: entry.word(),
                        file: list.file(),
                    },
                )
            })
            .collect::<Vec<_>>()
    };
    unknown
        .chain(stale(vocabulary.functions(), &called))
        .chain(stale(vocabulary.keywords(), &written))
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
        "create table values_table (\n    value_kind smallint,\n    constraint chk_values_table_presence check (num_nonnulls (value_kind) = 1)\n);\n";

    #[test]
    fn a_known_function_and_used_words_are_accepted() -> Result<(), FixtureError> {
        let known = vocabulary("num_nonnulls\n", "CHECK\n", &["num_nonnulls"]);
        assert_eq!(findings_with(Rule::FormatterVocabulary, &[(FILE, SQL)], &known)?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn an_unknown_function_and_stale_words_are_refused_where_each_is() -> Result<(), FixtureError> {
        let stale = vocabulary("sub_path_hash\n", "BRIN\n", &[]);
        assert_eq!(
            findings_with(Rule::FormatterVocabulary, &[(FILE, SQL)], &stale)?,
            ["migrations/branch/0001_values.sql:3:49", ".pg_format_functions:1:1", ".pg_format_keywords:1:1"]
        );
        Ok(())
    }
}
