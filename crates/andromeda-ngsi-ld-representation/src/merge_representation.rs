//! The representation a Merge Entity fragment is written in (Table 6.5.3.4-1).

use crate::response_format::ResponseFormat;
use thiserror::Error;

/// How the Entity Fragment of a clause 5.6.17 merge is written.
///
/// Table 6.5.3.4-1 admits the normalized representation of clause 4.5.1 and
/// the simplified one of clause 4.5.4 (`simplified`, or its synonym
/// `keyValues`); the concise representation is not a merge input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MergeRepresentation {
    /// Every attribute is a typed node (clause 4.5.1).
    #[default]
    Normalized,
    /// Every attribute is a bare value whose type the target entity already
    /// fixes (clause 4.5.4; Table 6.5.3.4-1 `format=simplified`).
    Simplified,
}

/// A representation token Table 6.5.3.4-1 does not admit for a merge.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum MergeRepresentationError {
    /// The concise representation is not a merge input.
    #[error("a merge fragment is written normalized or simplified, not {0}")]
    NotAMergeInput(ResponseFormat),
}

impl TryFrom<ResponseFormat> for MergeRepresentation {
    type Error = MergeRepresentationError;

    fn try_from(format: ResponseFormat) -> Result<MergeRepresentation, MergeRepresentationError> {
        match format {
            ResponseFormat::Normalized => Ok(MergeRepresentation::Normalized),
            ResponseFormat::Simplified | ResponseFormat::KeyValues => Ok(MergeRepresentation::Simplified),
            ResponseFormat::Concise => Err(MergeRepresentationError::NotAMergeInput(format)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        merge_representation::{MergeRepresentation, MergeRepresentationError},
        response_format::ResponseFormat,
    };

    #[test]
    fn normalized_and_simplified_are_merge_inputs_and_concise_is_not() {
        pretty_assertions::assert_eq!(MergeRepresentation::try_from(ResponseFormat::Normalized), Ok(MergeRepresentation::Normalized));
        pretty_assertions::assert_eq!(MergeRepresentation::try_from(ResponseFormat::Simplified), Ok(MergeRepresentation::Simplified));
        pretty_assertions::assert_eq!(MergeRepresentation::try_from(ResponseFormat::KeyValues), Ok(MergeRepresentation::Simplified));
        pretty_assertions::assert_eq!(
            MergeRepresentation::try_from(ResponseFormat::Concise),
            Err(MergeRepresentationError::NotAMergeInput(ResponseFormat::Concise))
        );
    }
}
