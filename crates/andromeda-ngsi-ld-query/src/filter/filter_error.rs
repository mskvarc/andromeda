//! Why a `q` or `csf` filter, or a `ValuePath` another parameter borrows from
//! clause 4.9, is outside the language.

use crate::{
    filter::{
        expectation::{Expectation, Found},
        language_range::{LanguageRange, LanguageRangeError},
        unicode_escape_error::UnicodeEscapeError,
    },
    pattern::pattern_error::PatternError,
};
use andromeda_ngsi_ld_lexicon::{
    reference::NgsiLdReferenceError,
    time::{date::NgsiLdDateError, date_time::NgsiLdDateTimeError, time_of_day::NgsiLdTimeError},
};
use andromeda_ngsi_ld_number::{JsonNumberError, held_text::UnholdableCharacter};
use langtag::{InvalidLangTag, LangTagBuf};
use std::num::NonZeroUsize;
use thiserror::Error;

/// The clause 4.9 rule a `q` or `csf` filter broke, or the broker limit it
/// passed.
#[derive(Debug, Error)]
pub enum FilterError {
    /// The q expression was empty.
    #[error("q expression: empty input")]
    Empty,
    /// A quoted string literal was not closed.
    #[error("q expression: unterminated string literal")]
    UnterminatedString,
    /// A backslash escape named a character the clause 4.9 `char` production
    /// does not define; IETF RFC 8259 section 7 lists exactly eight.
    #[error("q expression: invalid string escape '\\{escape}' at position {position}")]
    InvalidStringEscape {
        /// The character that followed the backslash.
        escape: char,
        /// Zero-based character offset of the backslash in the literal's body.
        position: usize,
    },
    /// A `\u` escape did not name a Unicode scalar value.
    #[error("q expression: invalid unicode escape at position {position}: {fault}")]
    InvalidUnicodeEscape {
        /// Zero-based character offset of the backslash in the literal's body.
        position: usize,
        /// The RFC 8259 rule the escape broke.
        fault: UnicodeEscapeError,
    },
    /// A quoted string carried a raw control character, which the clause 4.9
    /// `char` production requires to be written as an escape.
    #[error("q expression: unescaped control character U+{code_point:04X} at position {position}")]
    UnescapedControlCharacter {
        /// The code point that had to be escaped.
        code_point: u32,
        /// Zero-based character offset of the character in the literal's body.
        position: usize,
    },
    /// A token remained after the parser reached the end of the expression.
    #[error("q expression: unexpected {token} at position {position}")]
    UnexpectedToken {
        /// Zero-based offset into the token stream.
        position: usize,
        /// What remained.
        token: Found,
    },
    /// The parser expected a specific token but found something else.
    #[error("q expression: expected {expected}, found {found}")]
    Expected {
        /// What the grammar required at this position.
        expected: Expectation,
        /// What stood there instead.
        found: Found,
    },
    /// The last type of a linked-entity type guard is written bare and holds
    /// two colons, so which one ends the guard cannot be told.
    #[error("q expression: quote the entity type, so the colon ending the type guard is unambiguous")]
    AmbiguousTypeGuard,
    /// A character starts no token of the language.
    #[error("q expression: unexpected character '{character}' at position {position}")]
    UnexpectedCharacter {
        /// The character that starts no token.
        character: char,
        /// Zero-based character offset in the input.
        position: usize,
    },
    /// A numeric literal was lexed but failed to parse.
    #[error("q expression: invalid number literal: {0}")]
    InvalidNumber(#[source] JsonNumberError),
    /// An attribute path nests more segments under its leading name than the
    /// parser admits: sub-attributes, linked-entity hops and the members of a
    /// trailing group count alike.
    ///
    /// The limit is the depth a written attribute is admitted to, so a path
    /// beyond it addresses nothing a write can store and only grows the
    /// backend rendering with its depth.
    #[error("q expression: attribute path too deep ({depth} nested segments, limit {limit})")]
    PathTooDeep {
        /// The number of segments the path nests under its leading name.
        depth: usize,
        /// The maximum the parser accepts.
        limit: usize,
    },
    /// The filter makes more comparisons than the broker resolves in one
    /// query (clause 5.5.6, `TooComplexQuery`).
    #[error("q expression: {comparisons} comparisons, more than the {limit} this broker resolves in one query; a value list counts one comparison per value")]
    TooManyComparisons {
        /// The comparisons the filter makes, a value list making one per value.
        comparisons: usize,
        /// The most the broker resolves.
        limit: NonZeroUsize,
    },
    /// The filter nests its `;` and `|` combinations, or its parentheses,
    /// deeper than the broker resolves in one query (clause 5.5.6,
    /// `TooComplexQuery`).
    #[error("q expression: nested {depth} levels deep, more than the {limit} this broker resolves in one query")]
    NestedTooDeep {
        /// How deep the filter nests.
        depth: usize,
        /// The deepest the broker resolves.
        limit: NonZeroUsize,
    },
    /// A one-member bracket held a name that is neither an attribute name nor
    /// an RFC 5646 language tag, nor an RFC 4647 extended range.
    #[error("q expression: '{}' is neither a member name nor an RFC 5646 language tag ({range})", .tag.0)]
    InvalidTrailingMember {
        /// The language tag reading the bracket failed.
        #[source]
        tag: InvalidLangTag<String>,
        /// Why the bracket is no extended language range either.
        range: LanguageRangeError,
    },
    /// A bracket named a target language only a full RFC 5646 tag spells
    /// while the `q_target_languages` extension stays below `language-tags`:
    /// the clause 4.9 `AttrName` admits no `-`.
    #[error("q expression: the target language '{0}' is a full language tag, which needs the q_target_languages extension at language-tags")]
    LanguageTagNotAdmitted(LangTagBuf),
    /// A bracket held an RFC 4647 extended language range while the
    /// `q_target_languages` extension stays below `language-ranges`.
    #[error("q expression: the language range '{0}' needs the q_target_languages extension at language-ranges")]
    LanguageRangeNotAdmitted(LanguageRange),
    /// A bare word that is no `URI` stood as a value: clause 4.9 derives it
    /// from no production, whatever `expandValues` names.
    #[error("q expression: an unquoted word is no value; quote it")]
    BareWord,
    /// A bare literal written in the clause 4.6.3 `DateTime` form was not one.
    #[error("q expression: invalid date-time literal: {0}")]
    InvalidDateTime(#[source] NgsiLdDateTimeError),
    /// A bare literal written in the clause 4.6.3 `Date` form was not one.
    #[error("q expression: invalid date literal: {0}")]
    InvalidDate(#[source] NgsiLdDateError),
    /// A bare literal written in the clause 4.6.3 `Time` form was not one.
    #[error("q expression: invalid time literal: {0}")]
    InvalidTime(#[source] NgsiLdTimeError),
    /// A name of the filter, of a `csf`, of a path another parameter borrows,
    /// or of `jsonKeys` or `expandValues`, was not usable as an attribute name
    /// (clause 5.2.4).
    #[error("invalid attribute name: {0}")]
    InvalidAttributeName(#[source] NgsiLdReferenceError),
    /// The escapes of a `quotedStr` operand spell a character the broker
    /// cannot hold, so no stored value can be compared with it.
    #[error("a query text is refused: {0}")]
    UnholdableCharacter(#[from] UnholdableCharacter),
    /// The regular expression of a pattern operand was refused.
    #[error(transparent)]
    Pattern(#[from] PatternError),
}

#[cfg(test)]
mod tests {
    use crate::filter::{
        expectation::{Expectation, Found},
        filter_error::FilterError,
        parser::lexer::TokenKind,
    };

    #[test]
    fn a_missing_token_is_reported_by_kind_or_as_the_end_of_input() {
        let found = FilterError::Expected {
            expected: Expectation::Token(TokenKind::RParen),
            found: Found::Token(TokenKind::Semi),
        };
        pretty_assertions::assert_eq!(found.to_string(), "q expression: expected ), found ;");
        let ended = FilterError::Expected {
            expected: Expectation::Value,
            found: Found::EndOfInput,
        };
        pretty_assertions::assert_eq!(ended.to_string(), "q expression: expected value, found end of input");
    }

    #[test]
    fn a_trailing_token_is_reported_by_kind_and_position() {
        let error = FilterError::UnexpectedToken {
            position: 3,
            token: Found::Token(TokenKind::Ident),
        };
        pretty_assertions::assert_eq!(error.to_string(), "q expression: unexpected identifier at position 3");
    }

    #[test]
    fn a_path_too_deep_counts_its_segments() {
        pretty_assertions::assert_eq!(
            FilterError::PathTooDeep { depth: 9, limit: 8 }.to_string(),
            "q expression: attribute path too deep (9 nested segments, limit 8)"
        );
    }
}
