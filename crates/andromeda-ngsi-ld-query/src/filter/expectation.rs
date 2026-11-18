//! What a position in a `q` filter requires, and what it held instead.

use crate::filter::parser::lexer::{Token, TokenKind};
use strum::Display;

/// What the clause 4.9 grammar required where a parse failed.
///
/// A parse failure names this rather than echoing the client's text, so the
/// `BadRequestData` detail says which production the grammar was reading.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub enum Expectation {
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

/// What stood where a parse of a `q` filter failed.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub enum Found {
    /// A token of this kind.
    #[strum(to_string = "{0}")]
    Token(TokenKind),
    /// Nothing: the input ended.
    #[strum(serialize = "end of input")]
    EndOfInput,
}

impl Found {
    /// What a cursor position holds: the kind of its token, or the end of the
    /// input.
    #[must_use]
    pub fn of(token: Option<&Token>) -> Found {
        token.map_or(Found::EndOfInput, |token| Found::Token(TokenKind::from(token)))
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::{
        expectation::{Expectation, Found},
        parser::lexer::{Token, TokenKind},
    };

    #[test]
    fn a_found_token_is_named_by_kind_and_the_end_of_input_by_name() {
        pretty_assertions::assert_eq!(Found::of(Some(&Token::Ident("speed".to_owned()))).to_string(), "identifier");
        pretty_assertions::assert_eq!(Found::of(Some(&Token::LBrace)).to_string(), "{");
        pretty_assertions::assert_eq!(Found::of(None).to_string(), "end of input");
    }

    #[test]
    fn a_token_expectation_is_spelled_as_the_grammar_spells_the_token() {
        pretty_assertions::assert_eq!(Expectation::Token(TokenKind::RBrace).to_string(), "}");
        pretty_assertions::assert_eq!(Expectation::Token(TokenKind::Ident).to_string(), "identifier");
    }

    #[test]
    fn a_production_expectation_is_spelled_as_its_name() {
        pretty_assertions::assert_eq!(Expectation::Value.to_string(), "value");
        pretty_assertions::assert_eq!(Expectation::ComparableValue.to_string(), "comparable value");
        pretty_assertions::assert_eq!(Expectation::PatternString.to_string(), "pattern string");
        pretty_assertions::assert_eq!(Expectation::EntityType.to_string(), "entity type");
        pretty_assertions::assert_eq!(Expectation::GuardTerminator.to_string(), ":");
    }
}
