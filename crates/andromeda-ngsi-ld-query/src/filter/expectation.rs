//! What a position in a `q` expression requires, and what it held instead.

use crate::filter::lexer::{Token, TokenKind};
use strum::Display;

/// What the clause 4.9 grammar required where a parse failed.
///
/// A parse failure names this rather than echoing the client's text, so the
/// `BadRequestData` detail says which production the grammar was reading.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub enum QExpectation {
    /// One particular token.
    #[strum(to_string = "{0}")]
    Token(TokenKind),
    /// A comparison operand: a number, a quoted string, a boolean or a bare
    /// temporal literal.
    #[strum(serialize = "value")]
    Value,
    /// An operand of an ordering operator or a bound of a range: a number, a
    /// quoted string or a bare temporal literal.
    #[strum(serialize = "comparable value")]
    ComparableValue,
    /// The operand of `~=` or `!~=`.
    #[strum(serialize = "pattern string")]
    PatternString,
    /// A type name inside a linked-entity type guard.
    #[strum(serialize = "entity type")]
    EntityType,
    /// The colon that ends a linked-entity type guard.
    #[strum(serialize = ":")]
    GuardTerminator,
}

/// What stood where a parse of a `q` expression failed.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub enum QFound {
    /// A token of this kind.
    #[strum(to_string = "{0}")]
    Token(TokenKind),
    /// Nothing: the input ended.
    #[strum(serialize = "end of input")]
    EndOfInput,
}

impl QFound {
    /// What a cursor position holds: the kind of its token, or the end of the
    /// input.
    #[must_use]
    pub fn of(token: Option<&Token>) -> QFound {
        token.map_or(QFound::EndOfInput, |token| QFound::Token(TokenKind::from(token)))
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::{
        expectation::{QExpectation, QFound},
        lexer::{Token, TokenKind},
    };

    #[test]
    fn a_found_token_is_named_by_kind_and_the_end_of_input_by_name() {
        pretty_assertions::assert_eq!(QFound::of(Some(&Token::Ident("speed".to_owned()))).to_string(), "identifier");
        pretty_assertions::assert_eq!(QFound::of(Some(&Token::LBrace)).to_string(), "{");
        pretty_assertions::assert_eq!(QFound::of(None).to_string(), "end of input");
    }

    #[test]
    fn a_token_expectation_is_spelled_as_the_grammar_spells_the_token() {
        pretty_assertions::assert_eq!(QExpectation::Token(TokenKind::RBrace).to_string(), "}");
        pretty_assertions::assert_eq!(QExpectation::Token(TokenKind::Ident).to_string(), "identifier");
    }

    #[test]
    fn a_production_expectation_is_spelled_as_its_name() {
        pretty_assertions::assert_eq!(QExpectation::Value.to_string(), "value");
        pretty_assertions::assert_eq!(QExpectation::ComparableValue.to_string(), "comparable value");
        pretty_assertions::assert_eq!(QExpectation::PatternString.to_string(), "pattern string");
        pretty_assertions::assert_eq!(QExpectation::EntityType.to_string(), "entity type");
        pretty_assertions::assert_eq!(QExpectation::GuardTerminator.to_string(), ":");
    }
}
