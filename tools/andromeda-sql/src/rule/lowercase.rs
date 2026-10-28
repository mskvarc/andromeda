//! `lowercase`: no code token holds an uppercase letter, inside function
//! bodies too.
//!
//! `pg_format` lowercases only the words it knows, so an unknown keyword,
//! function or literal prefix keeps whatever case it was typed in. Comments,
//! the contents of string constants and quoted identifiers (`"C"`) are text,
//! not code, and keep their case.

use crate::{
    declared_schema::parsed_migration::SourceToken,
    rule::{Rule, RuleContext, RuleFinding},
};
use pg_query::protobuf::Token;
use std::ops::Range;

/// A code token with an uppercase letter.
#[derive(Debug, derive_more::Display)]
#[display("{_0} holds an uppercase letter")]
struct Uppercase<'schema>(&'schema str);

/// Every code token, or literal prefix, holding an uppercase letter.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    context
        .schema()
        .migrations()
        .iter()
        .flat_map(|migration| {
            migration.tokens().iter().filter_map(move |token| {
                let code = code_part(token, migration.token_text(token))?;
                let written = &migration.text()[code.clone()];
                written
                    .chars()
                    .any(char::is_uppercase)
                    .then(|| context.finding(Rule::Lowercase, migration.origin(code.start), code.len(), &Uppercase(written)))
            })
        })
        .collect()
}

/// The bytes of `token`, whose text is `written`, that are code: all of a
/// keyword, an identifier, an operator or a number; the prefix or dollar tag
/// of a string constant; nothing of a comment or a quoted identifier.
fn code_part(token: &SourceToken, written: &str) -> Option<Range<usize>> {
    let range = token.range();
    let kind = token.kind();
    if token.is_comment() || (kind == Token::Ident && written.starts_with('"')) {
        return None;
    }
    if !matches!(kind, Token::Sconst | Token::Usconst | Token::Bconst | Token::Xconst) {
        return Some(range);
    }
    let prefix = match written.strip_prefix('$') {
        Some(rest) => rest.find('$').map_or(written.len(), |tag_end| tag_end + 2),
        None => written.find('\'').unwrap_or(written.len()),
    };
    (prefix > 0).then(|| range.start..range.start + prefix)
}

#[cfg(test)]
mod tests {
    use crate::rule::{
        Rule,
        rule_fixture::{FixtureError, findings},
    };
    use pretty_assertions::assert_eq;

    const FILE: &str = "branch/0001_streams.sql";

    #[test]
    fn comments_strings_and_quoted_identifiers_keep_their_case() -> Result<(), FixtureError> {
        let sql = "-- Holds Streams.\ncreate table streams (\n    label text collate \"C\" default 'Unnamed',\n    mask bit(8) default b'0'::bit(8)\n);\n";
        assert_eq!(findings(Rule::Lowercase, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn an_uppercase_literal_prefix_and_body_keyword_are_refused() -> Result<(), FixtureError> {
        let sql = "create table streams (\n    mask bit(8) default B'0'::bit(8)\n);\n\ncreate function f ()\n    returns integer\n    language plpgsql\n    as $$\nBEGIN\n    return 1;\nend\n$$;\n";
        assert_eq!(
            findings(Rule::Lowercase, &[(FILE, sql)])?,
            ["migrations/branch/0001_streams.sql:2:25", "migrations/branch/0001_streams.sql:9:1"]
        );
        Ok(())
    }
}
