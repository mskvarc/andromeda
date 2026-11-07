//! The member names of an entity, for the clause 5.5.9 key-existence test.
//!
//! An `attrs` or `q` key-existence test may name `scope` and the entity's
//! `expiresAt` as well as an attribute, so the members of an entity are its
//! attributes plus those two when present.

use crate::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_vocabulary::expanded::properties;

/// One member an entity carries.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MemberName<'a> {
    /// `scope` (clause 4.18).
    Scope,
    /// The entity's `expiresAt` (clause 4.22).
    ExpiresAt,
    /// An attribute.
    Attribute(&'a AttributeName),
}

impl MemberName<'_> {
    /// The expanded name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            MemberName::Scope => properties::SCOPE.as_str(),
            MemberName::ExpiresAt => properties::EXPIRES_AT.as_str(),
            MemberName::Attribute(name) => name.as_str(),
        }
    }
}

impl EntityState {
    /// Every member the entity carries: `scope` and `expiresAt` when present,
    /// then the attributes in stored order.
    pub fn member_names(&self) -> impl Iterator<Item = MemberName<'_>> {
        let scope = self.scope().map(|_| MemberName::Scope);
        let expires_at = self.expires_at().map(|_| MemberName::ExpiresAt);
        scope.into_iter().chain(expires_at).chain(self.attributes().names().map(MemberName::Attribute))
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity_state::EntityState, reads::member_name::MemberName};
    use serde_json::json;

    fn state() -> EntityState {
        let document = json!([{
            "@id": "urn:ngsi-ld:E:1",
            "@type": ["https://ex.org/T"],
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/a"}],
            "https://uri.etsi.org/ngsi-ld/expiresAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2030-01-01T00:00:00Z"}],
            "https://ex.org/b": [
                {"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]},
                {"@type": ["https://uri.etsi.org/ngsi-ld/Relationship"], "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:x:y"}], "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:d"}]}
            ]
        }]);
        EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a state")
    }

    #[test]
    fn the_member_names_include_the_scope_and_the_expiry_beside_the_attributes() {
        let state = state();
        let names: Vec<String> = state.member_names().map(|name| name.as_str().to_owned()).collect();
        pretty_assertions::assert_eq!(
            names,
            vec![
                "https://uri.etsi.org/ngsi-ld/scope",
                "https://uri.etsi.org/ngsi-ld/expiresAt",
                "https://ex.org/b"
            ]
        );
        assert!(state.member_names().any(|name| matches!(name, MemberName::Attribute(_))));
    }
}
