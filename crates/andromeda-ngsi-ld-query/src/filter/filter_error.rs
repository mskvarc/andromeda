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
    identifier_length::IdentifierTooLong,
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
    /// The filter text was empty.
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
        /// Zero-based byte offset of the backslash in the text the parser read.
        position: usize,
    },
    /// A `\u` escape did not name a Unicode scalar value.
    #[error("q expression: invalid unicode escape at position {position}: {error}")]
    InvalidUnicodeEscape {
        /// Zero-based byte offset of the backslash in the text the parser read.
        position: usize,
        /// The RFC 8259 rule the escape broke.
        error: UnicodeEscapeError,
    },
    /// A quoted string carried a raw control character, which the clause 4.9
    /// `char` production requires to be written as an escape.
    #[error("q expression: unescaped control character U+{code_point:04X} at position {position}")]
    UnescapedControlCharacter {
        /// The code point that had to be escaped.
        code_point: u32,
        /// Zero-based byte offset of the character in the text the parser read.
        position: usize,
    },
    /// The grammar required something and found something else.
    #[error("q expression: expected {expected}, found {found} at position {position}")]
    Expected {
        /// What the grammar required at this position.
        expected: Expectation,
        /// What stood there instead.
        found: Found,
        /// Zero-based byte offset in the text the parser read where the grammar required it.
        position: usize,
    },
    /// An unquoted name is directly followed by a colon where no hop's type
    /// list ends. An unquoted name is a clause 4.9 `AttrName`, which holds no
    /// colon, so a prefixed name or an absolute IRI is written in double quotes.
    #[error("q expression: the name at position {position} holds ':' and goes in double quotes")]
    UnquotedColonName {
        /// Zero-based byte offset of the name's first character in the text the parser read.
        position: usize,
    },
    /// Input continues where the filter, or a value path read on its own, is
    /// complete.
    #[error("q expression: unexpected character '{character}' at position {position}")]
    UnexpectedCharacter {
        /// The first character of what remains.
        character: char,
        /// Zero-based byte offset in the text the parser read.
        position: usize,
    },
    /// A number operand is outside the RFC 8259 `number` production.
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
    /// A name of the filter, of a `csf`, of a value path another parameter borrows,
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

// Hand-written: `#[from]` converts a whole variant payload, and this one wraps
// the bound refusal in the reference error the variant carries.
impl From<IdentifierTooLong> for FilterError {
    /// An expanded name past the identifier bound is an unusable attribute
    /// name.
    fn from(too_long: IdentifierTooLong) -> FilterError {
        FilterError::InvalidAttributeName(too_long.into())
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::{
        expectation::{Expectation, Found, Punctuation},
        filter_error::FilterError,
    };

    #[test]
    fn a_missing_punctuation_is_reported_with_what_stood_there() {
        let found = FilterError::Expected {
            expected: Expectation::Punctuation(Punctuation::CloseParen),
            found: Found::Character(';'),
            position: 4,
        };
        pretty_assertions::assert_eq!(found.to_string(), "q expression: expected ), found ';' at position 4");
        let ended = FilterError::Expected {
            expected: Expectation::Value,
            found: Found::EndOfInput,
            position: 3,
        };
        pretty_assertions::assert_eq!(ended.to_string(), "q expression: expected value, found end of input at position 3");
    }

    #[test]
    fn trailing_input_is_reported_by_character_and_position() {
        let error = FilterError::UnexpectedCharacter { character: 'b', position: 5 };
        pretty_assertions::assert_eq!(error.to_string(), "q expression: unexpected character 'b' at position 5");
    }

    #[test]
    fn a_path_too_deep_counts_its_segments() {
        pretty_assertions::assert_eq!(
            FilterError::PathTooDeep { depth: 9, limit: 8 }.to_string(),
            "q expression: attribute path too deep (9 nested segments, limit 8)"
        );
    }
}
