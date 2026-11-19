//! The stored tree an expanded projection term is serialized as.

use crate::projection::{expanded_projection_term::ExpandedProjectionTerm, projection_error::ProjectionError};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, reference::NgsiLdReferenceError};
use iri_rs::IriBuf;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// One term of a stored projection list as the tree `{"name": ..., "linked": [...]}`.
///
/// A stored subscription keeps its terms expanded (clause 5.5.7), and an expanded IRI may hold the `,` and `|` the clause 4.21 grammar separates terms with, so the terms are not stored as clause 4.21 text. `name` is a keyword (`id`, `type`, `scope`) or an absolute IRI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredProjectionTerm {
    /// The keyword or the expanded IRI this term selects.
    name: String,
    /// The linked-entity sub-projection, empty when the term selects no linked entity.
    linked: Vec<StoredProjectionTerm>,
}

impl From<ExpandedProjectionTerm> for StoredProjectionTerm {
    fn from(term: ExpandedProjectionTerm) -> StoredProjectionTerm {
        StoredProjectionTerm {
            name: term.to_string(),
            linked: term.linked_terms().iter().cloned().map(StoredProjectionTerm::from).collect(),
        }
    }
}

impl TryFrom<StoredProjectionTerm> for ExpandedProjectionTerm {
    type Error = ProjectionError;

    fn try_from(stored: StoredProjectionTerm) -> Result<ExpandedProjectionTerm, ProjectionError> {
        if let Some(keyword) = ExpandedProjectionTerm::keyword(&stored.name) {
            return if stored.linked.is_empty() {
                Ok(keyword)
            } else {
                Err(ProjectionError::KeywordWithLinkedTerm {
                    keyword: AttributeName::from_str(&stored.name).map_err(ProjectionError::InvalidAttributeName)?,
                })
            };
        }
        let iri = IriBuf::new(stored.name).map_err(|_| ProjectionError::InvalidAttributeName(NgsiLdReferenceError::NotAbsolute))?;
        let linked = stored.linked.into_iter().map(ExpandedProjectionTerm::try_from).collect::<Result<_, _>>()?;
        Ok(ExpandedProjectionTerm::Attribute { iri, linked })
    }
}

#[cfg(test)]
mod tests {
    use crate::projection::expanded_projection_term::ExpandedProjectionTerm;
    use serde_json::json;

    #[test]
    fn a_term_tree_round_trips_as_the_name_and_linked_members() {
        let stored = json!([{"name": "id", "linked": []}, {"name": "https://example.com/owner", "linked": [{"name": "scope", "linked": []}]}]);
        let terms: Vec<ExpandedProjectionTerm> = serde_json::from_value(stored.clone()).expect("a stored tree");
        pretty_assertions::assert_eq!(serde_json::to_value(&terms).expect("writes"), stored);
    }

    #[test]
    fn a_stored_name_that_is_no_keyword_or_absolute_iri_is_refused() {
        assert!(serde_json::from_value::<ExpandedProjectionTerm>(json!({"name": "owner", "linked": []})).is_err());
    }

    #[test]
    fn a_stored_keyword_with_a_linked_term_is_refused() {
        assert!(serde_json::from_value::<ExpandedProjectionTerm>(json!({"name": "id", "linked": [{"name": "type", "linked": []}]})).is_err());
    }
}
