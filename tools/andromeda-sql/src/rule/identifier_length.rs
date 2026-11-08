//! `identifier-length`: every identifier a migration writes fits the 63 bytes
//! PostgreSQL keeps of one.
//!
//! A longer identifier is truncated without an error, so two names that
//! differ only past the limit collide, and the name the code uses does not
//! match the one the catalogue holds. The parser reports the truncated name,
//! so the rule reads the identifiers as written, which covers every object
//! kind, and every name a statement refers to as well as creates.

use crate::rule::{Rule, RuleContext, RuleFinding};
use pg_query::protobuf::Token;

/// The longest identifier PostgreSQL keeps whole, in bytes.
const IDENTIFIER_BYTE_LIMIT: usize = 63;

/// An identifier past the limit.
#[derive(Debug, derive_more::Display)]
#[display("the identifier {name} is {length} bytes, which PostgreSQL truncates to {IDENTIFIER_BYTE_LIMIT}")]
struct LongIdentifier<'schema> {
    /// The identifier as written.
    name: &'schema str,
    /// Its length in bytes, quotes left out.
    length: usize,
}

/// Every identifier longer than PostgreSQL keeps.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    context
        .schema()
        .migrations()
        .iter()
        .flat_map(|migration| {
            migration.tokens().iter().filter(|token| token.kind() == Token::Ident).filter_map(move |token| {
                let name = migration.token_text(token);
                let length = name.strip_prefix('"').and_then(|quoted| quoted.strip_suffix('"')).map_or(name.len(), str::len);
                (length > IDENTIFIER_BYTE_LIMIT).then(|| {
                    context.finding(
                        Rule::IdentifierLength,
                        migration.origin(token.range().start),
                        name.len(),
                        &LongIdentifier { name, length },
                    )
                })
            })
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

    const FILE: &str = "tenant/0001_entities.sql";

    #[test]
    fn a_name_of_63_bytes_is_accepted() -> Result<(), FixtureError> {
        let sql = format!(
            "create table entities (\n    entity_key bigint not null\n);\n\ncreate index {} on entities (entity_key);\n",
            "i".repeat(63)
        );
        assert_eq!(findings(Rule::IdentifierLength, &[(FILE, &sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn an_index_name_of_64_bytes_is_refused_at_the_name() -> Result<(), FixtureError> {
        let sql = format!(
            "create table entities (\n    entity_key bigint not null\n);\n\ncreate index {} on entities (entity_key);\n",
            "i".repeat(64)
        );
        assert_eq!(findings(Rule::IdentifierLength, &[(FILE, &sql)])?, ["migrations/tenant/0001_entities.sql:5:14"]);
        Ok(())
    }
}
