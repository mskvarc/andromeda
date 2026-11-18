//! Why an entity type selection is outside clause 4.17.

use crate::query_text::disjunctive_form::DisjunctiveFormErrors;
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReferenceError;
use thiserror::Error;

/// The clause 4.17 rule an entity type selection broke.
#[derive(Debug, Error)]
pub enum TypeSelectionError {
    /// The type selection text was empty.
    #[error("type expression: empty input")]
    Empty,
    /// The type selection ended before a primary could be parsed.
    #[error("type expression: unexpected end of input")]
    UnexpectedEnd,
    /// A `)` was expected but not found.
    #[error("type expression: unbalanced parenthesis at position {position}")]
    UnbalancedParenthesis {
        /// Zero-based byte offset where the `)` was required, into the type selection with its surrounding whitespace trimmed.
        position: usize,
    },
    /// A character outside the type-name grammar was encountered.
    #[error("type expression: unexpected character '{character}' at position {position}")]
    UnexpectedCharacter {
        /// The offending character.
        character: char,
        /// Zero-based byte offset of the character, into the type selection with its surrounding whitespace trimmed.
        position: usize,
    },
    /// A type name did not satisfy the clause 4.6.2 name grammar.
    #[error("type expression: invalid type name at position {position}: {source}")]
    InvalidName {
        /// Zero-based byte offset of the name, into the type selection with its surrounding whitespace trimmed.
        position: usize,
        /// The clause 4.6.2 rule the name broke.
        #[source]
        source: NgsiLdReferenceError,
    },
}

impl DisjunctiveFormErrors for TypeSelectionError {
    fn missing_item(found: Option<char>, position: usize) -> Self {
        match found {
            Some(character) => TypeSelectionError::UnexpectedCharacter { character, position },
            None => TypeSelectionError::UnexpectedEnd,
        }
    }

    fn group_in_group(position: usize) -> Self {
        TypeSelectionError::UnexpectedCharacter { character: '(', position }
    }

    fn nested_group(position: usize) -> Self {
        TypeSelectionError::UnbalancedParenthesis { position }
    }

    fn unclosed_group(position: usize) -> Self {
        TypeSelectionError::UnbalancedParenthesis { position }
    }

    fn ungrouped_conjunction(position: usize) -> Self {
        TypeSelectionError::UnexpectedCharacter { character: ';', position }
    }

    fn trailing_input(character: char, position: usize) -> Self {
        TypeSelectionError::UnexpectedCharacter { character, position }
    }
}
