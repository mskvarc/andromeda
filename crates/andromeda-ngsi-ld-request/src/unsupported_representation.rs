//! Why an operation does not support a `format` or `options` value.

use crate::options_token::OptionsToken;
use thiserror::Error;

/// The reason a representation parameter's value is outside its operation's
/// grammar, which clause 6.3.20 answers with `InvalidRequest`.
#[derive(Clone, Copy, Debug, Eq, Error, Hash, PartialEq)]
pub enum UnsupportedRepresentation {
    /// A `format` value names no rendering this operation produces.
    #[error("the value names no representation this operation renders")]
    UnknownFormat,
    /// The `options` list is empty or holds an empty token.
    #[error("options must contain non-empty comma-separated tokens")]
    EmptyOptionsToken,
    /// An `options` token names nothing the specification defines.
    #[error("an options token names nothing the specification defines")]
    UnknownOptionsToken,
    /// An `options` token the specification defines, but not for this
    /// operation.
    #[error("options token '{0}' is not supported by this operation")]
    UnsupportedOptionsToken(OptionsToken),
    /// An `options` token appears twice.
    #[error("duplicate options token '{0}'")]
    DuplicateOptionsToken(OptionsToken),
    /// Two `options` tokens name different renderings.
    #[error("options tokens name conflicting representations")]
    ConflictingRepresentations,
}

#[cfg(test)]
mod tests {
    use crate::{options_token::OptionsToken, unsupported_representation::UnsupportedRepresentation};

    #[test]
    fn a_refusal_names_the_token_at_fault_when_it_is_one_the_specification_defines() {
        pretty_assertions::assert_eq!(
            UnsupportedRepresentation::DuplicateOptionsToken(OptionsToken::SysAttrs).to_string(),
            "duplicate options token 'sysAttrs'"
        );
        pretty_assertions::assert_eq!(
            UnsupportedRepresentation::UnsupportedOptionsToken(OptionsToken::NoOverwrite).to_string(),
            "options token 'noOverwrite' is not supported by this operation"
        );
    }
}
