//! What a position in a `q` filter requires, and what it held instead.

use strum::Display;

/// What the clause 4.9 grammar required where a parse failed.
///
/// A parse failure names this rather than echoing the client's text, so the
/// `BadRequestData` detail says which production the grammar was reading.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub enum Expectation {
    /// The name of an attribute, quoted or bare.
    #[strum(serialize = "attribute name")]
    AttributeName,
    /// One closing punctuation mark.
    #[strum(to_string = "{0}")]
    Punctuation(Punctuation),
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

/// A punctuation mark that closes a construct the filter opened.
///
/// No comma is listed: the type guard reads a comma only after seeing one, so
/// no refusal expects it.
#[derive(Clone, Copy, Debug, derive_more::Display, Eq, PartialEq)]
pub enum Punctuation {
    /// `)`, closing a parenthesised group.
    #[display(")")]
    CloseParen,
    /// `]`, closing a trailing group.
    #[display("]")]
    CloseBracket,
    /// `}`, closing a linked-entity hop.
    #[display("}}")]
    CloseBrace,
}

/// What stood where a parse of a `q` filter failed.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub enum Found {
    /// This character.
    #[strum(to_string = "'{0}'")]
    Character(char),
    /// Nothing: the input ended.
    #[strum(serialize = "end of input")]
    EndOfInput,
}

impl Found {
    /// What a position holds: its character, or the end of the input.
    #[must_use]
    pub const fn of(character: Option<char>) -> Found {
        match character {
            Some(character) => Found::Character(character),
            None => Found::EndOfInput,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::expectation::{Expectation, Found, Punctuation};

    #[test]
    fn a_found_character_is_quoted_and_the_end_of_input_is_named() {
        pretty_assertions::assert_eq!(Found::of(Some('s')).to_string(), "'s'");
        pretty_assertions::assert_eq!(Found::of(Some('{')).to_string(), "'{'");
        pretty_assertions::assert_eq!(Found::of(None).to_string(), "end of input");
    }

    #[test]
    fn a_punctuation_expectation_is_spelled_as_its_character() {
        pretty_assertions::assert_eq!(Expectation::Punctuation(Punctuation::CloseParen).to_string(), ")");
        pretty_assertions::assert_eq!(Expectation::Punctuation(Punctuation::CloseBracket).to_string(), "]");
        pretty_assertions::assert_eq!(Expectation::Punctuation(Punctuation::CloseBrace).to_string(), "}");
    }

    #[test]
    fn a_production_expectation_is_spelled_as_its_name() {
        pretty_assertions::assert_eq!(Expectation::AttributeName.to_string(), "attribute name");
        pretty_assertions::assert_eq!(Expectation::Value.to_string(), "value");
        pretty_assertions::assert_eq!(Expectation::ComparableValue.to_string(), "comparable value");
        pretty_assertions::assert_eq!(Expectation::PatternString.to_string(), "pattern string");
        pretty_assertions::assert_eq!(Expectation::EntityType.to_string(), "entity type");
        pretty_assertions::assert_eq!(Expectation::GuardTerminator.to_string(), ":");
    }
}
