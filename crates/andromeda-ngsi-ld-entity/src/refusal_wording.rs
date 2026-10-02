//! How a refusal names what it refuses in client prose.
//!
//! The messages name an attribute or member by its core-context term where
//! one exists and by its expanded IRI otherwise, and never quote raw input.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
use andromeda_ngsi_ld_vocabulary::expanded_to_compact;

/// The `attribute 'x': ` prefix of a message about one attribute's member.
pub(crate) fn attribute_prefix(attribute: Option<&AttributeName>) -> String {
    attribute.map_or_else(String::new, |attribute| format!("attribute '{attribute}': "))
}

/// Render the subject of a forbidden-member message from its typed parts.
///
/// Named after the attribute where the instance has one, and after its family's
/// fragment otherwise, matching how the specification phrases the prohibition.
pub(crate) fn forbidden_member_subject(family: Option<&AttributeType>, attribute: Option<&AttributeName>) -> String {
    match (family, attribute) {
        (Some(family), Some(attribute)) => format!("{family} '{attribute}'"),
        (Some(family), None) => format!("the {family} fragment"),
        (None, Some(attribute)) => format!("attribute '{attribute}'"),
        (None, None) => "the attribute fragment".to_owned(),
    }
}

/// Name an attribute or member by its compact term where the core context
/// defines one, and by its expanded IRI otherwise.
pub(crate) fn compact_term(name: &AttributeName) -> &str {
    expanded_to_compact(name.as_str()).unwrap_or_else(|| name.as_str())
}
