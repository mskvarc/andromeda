//! The name of a stored projection term.

use iri_rs::IriBuf;
use serde::{Deserialize, Serialize};

/// What one term of a stored projection list selects: a clause 4.21 keyword or the expanded IRI of an attribute.
///
/// Serialized as the bare string, so the stored text is the keyword or the IRI and nothing around it. An expanded IRI has a scheme and no keyword has one, so the two never read as each other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum StoredProjectionName {
    /// The entity identifier.
    #[serde(rename = "id")]
    Id,
    /// The entity type.
    #[serde(rename = "type")]
    Type,
    /// The entity scope.
    #[serde(rename = "scope")]
    Scope,
    /// An attribute, by its expanded IRI.
    #[serde(untagged)]
    Attribute(IriBuf),
}

#[cfg(test)]
mod tests {
    use crate::projection::stored_projection_name::StoredProjectionName;
    use iri_rs::IriBuf;
    use serde_json::json;

    #[test]
    fn a_keyword_is_stored_as_its_spelling() {
        for (name, text) in [
            (StoredProjectionName::Id, "id"),
            (StoredProjectionName::Type, "type"),
            (StoredProjectionName::Scope, "scope"),
        ] {
            pretty_assertions::assert_eq!(serde_json::to_value(&name).expect("writes"), json!(text));
            pretty_assertions::assert_eq!(serde_json::from_value::<StoredProjectionName>(json!(text)).expect("reads"), name);
        }
    }

    #[test]
    fn an_attribute_is_stored_as_its_iri() {
        let iri = IriBuf::new("https://example.com/speed".to_owned()).expect("valid IRI");
        let name = StoredProjectionName::Attribute(iri);

        pretty_assertions::assert_eq!(serde_json::to_value(&name).expect("writes"), json!("https://example.com/speed"));
        pretty_assertions::assert_eq!(
            serde_json::from_value::<StoredProjectionName>(json!("https://example.com/speed")).expect("reads"),
            name
        );
    }

    #[test]
    fn a_text_that_is_no_keyword_or_absolute_iri_is_refused() {
        for text in [json!("owner"), json!("Id"), json!(""), json!(5)] {
            assert!(serde_json::from_value::<StoredProjectionName>(text.clone()).is_err(), "{text}");
        }
    }
}
