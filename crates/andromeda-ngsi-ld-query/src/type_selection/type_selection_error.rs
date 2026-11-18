//! Why an entity type selection is outside clause 4.17.

use andromeda_ngsi_ld_lexicon::reference::NgsiLdReferenceError;
use thiserror::Error;

/// The clause 4.17 rule an entity type selection broke.
#[derive(Debug, Error)]
pub enum TypeSelectionError {
    /// The type expression string was empty.
    #[error("type expression: empty input")]
    Empty,
    /// The type expression ended before a primary could be parsed.
    #[error("type expression: unexpected end of input")]
    UnexpectedEnd,
    /// A `)` was expected but not found.
    #[error("type expression: unbalanced parenthesis at position {position}")]
    UnbalancedParenthesis {
        /// Zero-based byte offset in the input where the `)` was required.
        position: usize,
    },
    /// A character outside the type-name grammar was encountered.
    #[error("type expression: unexpected character '{character}' at position {position}")]
    UnexpectedCharacter {
        /// The offending character.
        character: char,
        /// Zero-based byte offset of the character in the input.
        position: usize,
    },
    /// A type name did not satisfy the clause 4.6.2 name grammar.
    #[error("type expression: invalid type name at position {position}: {source}")]
    InvalidName {
        /// Zero-based byte offset of the name in the input.
        position: usize,
        /// The clause 4.6.2 rule the name broke.
        #[source]
        source: NgsiLdReferenceError,
    },
}
