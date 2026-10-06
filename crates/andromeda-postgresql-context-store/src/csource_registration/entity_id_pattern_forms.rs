//! The portable restatement of every `idPattern` a registration declares,
//! stored beside the `information` that declares it.
//!
//! A request's own pattern reaches PostgreSQL as a bind value, so the broker
//! hands over the
//! [`PortablePattern`](andromeda_ngsi_ld_query::portable_pattern::PortablePattern)
//! in place of the client's text. A registration's `idPattern` does not:
//! whether the registration covers a concrete entity (clause 5.10.2) and
//! whether a discovery request's identifiers match it (clause 5.12) are both
//! decided inside one statement that reads the pattern out of the stored
//! `information`, entry by entry. The restatement is therefore persisted as a
//! JSON object keyed by the pattern as written, which is what that statement
//! reads out of the entry it is testing, so the lookup names exactly the
//! restatement of that entry's pattern.
//!
//! Like the routing projection it is derived from the typed `information` and
//! written in the same statement as the document, and nothing in the database
//! derives it. It is stored with the [`RenderingRevision`] that produced it, so
//! a broker whose rendering differs -- a fix to the renderer, or a
//! `regex-syntax` upgrade with other case-fold tables -- renders it again when
//! it opens the tenant
//! ([`crate::csource_registration::pattern_forms_refresh`]).

use andromeda_ngsi_ld_model::registration::info::RegistrationInfo;
use andromeda_ngsi_ld_query::portable_pattern::{PortablePattern, RenderingRevision};
use indexmap::IndexMap;
use serde::{Serialize, Serializer};

/// What a registration stores about its identifier patterns.
#[derive(Debug, PartialEq)]
pub(crate) struct StoredPatternForms<'a> {
    /// Each pattern as written, mapped to its restatement.
    pub(crate) forms: IndexMap<&'a str, Restatement<'a>>,
    /// The rendering revision that produced [`StoredPatternForms::forms`].
    pub(crate) revision: RenderingRevision,
}

/// The restatement of one pattern, written as the text of its portable form.
#[derive(Debug, PartialEq)]
pub(crate) struct Restatement<'a>(&'a PortablePattern);

// Hand-written: `PortablePattern` is a foreign type without a serde
// implementation, and a derive on this newtype would write a wrapper object.
impl Serialize for Restatement<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0.as_str())
    }
}

/// The identifier-pattern restatements for a registration's `information`,
/// rendered by this build, or `None` when it declares no `idPattern`.
///
/// Two entries declaring the same pattern share one member: the same text has
/// the same restatement.
pub(crate) fn stored_pattern_forms(information: &[RegistrationInfo]) -> Option<StoredPatternForms<'_>> {
    let forms: IndexMap<&str, Restatement<'_>> = information
        .iter()
        .filter_map(|entry| entry.entities.as_deref())
        .flatten()
        .filter_map(|selector| selector.id_pattern.as_ref())
        .map(|pattern| (pattern.as_str(), Restatement(pattern.portable())))
        .collect();
    (!forms.is_empty()).then_some(StoredPatternForms {
        forms,
        revision: PortablePattern::RENDERING_REVISION,
    })
}

#[cfg(test)]
mod tests {
    use crate::csource_registration::entity_id_pattern_forms::stored_pattern_forms;
    use andromeda_ngsi_ld_model::registration::info::RegistrationInfo;
    use andromeda_ngsi_ld_query::portable_pattern::PortablePattern;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn information(entries: serde_json::Value) -> Vec<RegistrationInfo> {
        serde_json::from_value(entries).expect("valid information")
    }

    /// Every declared pattern maps from its text to its restatement, across
    /// entries and across the selectors of one entry, stamped with this
    /// build's rendering revision.
    #[test]
    fn every_declared_pattern_maps_from_its_text_to_its_restatement() {
        let information = information(json!([
            {"entities": [{"type": "Bee", "idPattern": "(?i)^urn:ngsi-ld:čebela:"}, {"type": "Hive", "id": "urn:ngsi-ld:Hive:1"}]},
            {"entities": [{"type": "Vehicle", "idPattern": "^urn:ngsi-ld:Vehicle:.*"}], "propertyNames": ["speed"]}
        ]));
        let stored = stored_pattern_forms(&information).expect("patterns are declared");
        assert_eq!(
            serde_json::to_value(&stored.forms).expect("the forms write"),
            json!({
                "(?i)^urn:ngsi-ld:čebela:": "^[Uu][Rr][Nn]:[Nn][Gg][Ssſ][Ii]-[Ll][Dd]:[Čč][Ee][Bb][Ee][Ll][Aa]:",
                "^urn:ngsi-ld:Vehicle:.*": "^urn:ngsi-ld:Vehicle:.*"
            })
        );
        assert_eq!(stored.revision, PortablePattern::RENDERING_REVISION);
    }

    /// The same pattern declared twice is stored once.
    #[test]
    fn a_pattern_declared_by_two_entries_is_stored_once() {
        let information = information(json!([
            {"entities": [{"type": "Bee", "idPattern": "(?i)čebela"}]},
            {"entities": [{"type": "Hive", "idPattern": "(?i)čebela"}]}
        ]));
        assert_eq!(
            stored_pattern_forms(&information).map(|stored| serde_json::to_value(&stored.forms).expect("the forms write")),
            Some(json!({"(?i)čebela": "[Čč][Ee][Bb][Ee][Ll][Aa]"}))
        );
    }

    /// A registration naming its entities by identifier, by type alone or not
    /// at all has nothing to restate, and stores `NULL` for both the forms and
    /// their revision.
    #[test]
    fn a_registration_without_an_identifier_pattern_stores_nothing() {
        let information = information(json!([
            {"entities": [{"type": "Hive", "id": "urn:ngsi-ld:Hive:1"}, {"type": "Bee"}]},
            {"propertyNames": ["speed"]}
        ]));
        assert_eq!(stored_pattern_forms(&information), None);
        assert_eq!(stored_pattern_forms(&[]), None);
    }
}
