//! Validation for the deprecated `attrs` selection parameter.

use crate::error::ParameterError;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

/// Validate that `attrs` contains only attribute names.
///
/// Tables 6.4.3.2-1, 6.5.3.1-1 and 6.19.3.1-1 explicitly exclude the Entity
/// members `id`, `type` and `scope` from this deprecated synonym for `pick`.
pub(crate) fn validate_attrs(attrs: Option<&[AttributeName]>) -> Result<(), ParameterError> {
    let reserved = attrs
        .into_iter()
        .flatten()
        .find(|attribute| matches!(attribute.as_str(), "id" | "type" | "scope"));
    match reserved {
        Some(attribute) => Err(ParameterError::AttrsReservedMember(attribute.clone())),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{attribute_selection::validate_attrs, error::ParameterError};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

    #[test]
    fn entity_members_are_rejected_as_attrs() {
        let attrs = ["speed".parse::<AttributeName>().expect("attribute"), "id".parse().expect("name")];
        assert!(matches!(validate_attrs(Some(&attrs)), Err(ParameterError::AttrsReservedMember(member)) if member.as_str() == "id"));
    }

    #[test]
    fn genuine_attribute_names_are_accepted() {
        let attrs = ["speed".parse::<AttributeName>().expect("attribute")];
        assert!(validate_attrs(Some(&attrs)).is_ok());
    }
}
