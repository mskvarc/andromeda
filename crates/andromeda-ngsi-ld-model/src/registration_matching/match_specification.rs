//! What a request specifies for matching registrations (clause 5.12), and the
//! registrations and blocks it matches.

use crate::{
    registration::{ContextSourceRegistration, info::RegistrationInfo},
    registration_matching::entity_specification::EntitySpecification,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

/// What a request specifies for matching (clause 5.12): its entity
/// specifications, any of which may match, and the attribute names it asks for.
#[derive(Debug, Clone, Copy)]
pub struct RegistrationMatchSpecification<'spec> {
    /// The entity specifications; empty when the request specifies no entity.
    pub entities: &'spec [EntitySpecification<'spec>],
    /// The attribute names, expanded; empty means every attribute is requested.
    pub attributes: &'spec [AttributeName],
}

impl RegistrationMatchSpecification<'_> {
    /// Whether some `RegistrationInfo` of `registration` matches: the
    /// registration covers what the request specifies.
    #[must_use]
    pub fn covers(&self, registration: &ContextSourceRegistration) -> bool {
        self.matching_information(&registration.coverage.information).next().is_some()
    }

    /// Whether one `RegistrationInfo` matches (clause 5.12): the entity axis and
    /// the attribute axis both hold on the same element.
    #[must_use]
    pub fn matches_information(&self, info: &RegistrationInfo) -> bool {
        self.entity_axis_matches(info) && self.attribute_axis_matches(info)
    }

    /// The `RegistrationInfo` elements of `information` that match, in order.
    ///
    /// This is the trimming clauses 5.10.2.5 and 5.11.7 ask for: a returned
    /// registration carries only the matching elements.
    pub fn matching_information<'info>(&self, information: &'info [RegistrationInfo]) -> impl Iterator<Item = &'info RegistrationInfo> {
        information.iter().filter(|info| self.matches_information(info))
    }

    /// Clause 5.12: a request specifying no entity matches every element;
    /// otherwise some specification must match the element's entity axis.
    fn entity_axis_matches(&self, info: &RegistrationInfo) -> bool {
        self.entities.is_empty() || self.entities.iter().any(|specification| specification.matches_entity_axis(info))
    }

    /// Clause 5.12: no attribute names asked for, or no property and no
    /// relationship registered, or at least one asked-for name registered.
    fn attribute_axis_matches(&self, info: &RegistrationInfo) -> bool {
        if self.attributes.is_empty() {
            return true;
        }
        let mut declared = info.attribute_names().peekable();
        if declared.peek().is_none() {
            return true;
        }
        declared.any(|name| self.attributes.contains(name))
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        registration::{ContextSourceRegistration, info::RegistrationInfo},
        registration_matching::{
            entity_specification::{EntitySpecification, TypeConstraint},
            match_specification::RegistrationMatchSpecification,
        },
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
    use andromeda_ngsi_ld_query::{pattern::Pattern, type_selection::grammar::TypeExprRaw};
    use serde_json::json;
    use std::{ptr, slice};

    fn info(value: serde_json::Value) -> RegistrationInfo {
        serde_json::from_value(value).expect("information parses")
    }

    fn registration(information: &serde_json::Value) -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:csr:1",
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "information": information,
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid registration")
    }

    fn uri(value: &str) -> Identifier {
        value.parse().expect("valid URI")
    }

    fn name(value: &str) -> AttributeName {
        value.parse().expect("valid attribute name")
    }

    fn type_selector(raw: &str) -> TypeExprRaw {
        raw.parse().expect("valid type selector")
    }

    fn pattern(raw: &str) -> Pattern {
        Pattern::parse(raw).expect("valid pattern")
    }

    #[test]
    fn a_specified_id_takes_precedence_over_a_specified_id_pattern() {
        let element = info(json!({"entities": [{"id": "urn:ngsi-ld:A:1", "type": "A"}]}));
        let nomatch = pattern("^urn:nomatch");
        let ids = [uri("urn:ngsi-ld:A:1")];
        let entities = [EntitySpecification {
            types: TypeConstraint::Any,
            ids: &ids,
            id_pattern: Some(&nomatch),
        }];
        let specification = RegistrationMatchSpecification {
            entities: &entities,
            attributes: &[],
        };
        assert!(specification.matches_information(&element));
    }

    #[test]
    fn attribute_names_match_when_none_asked_none_registered_or_one_shared() {
        let selector = type_selector("Vehicle");
        let entities = [EntitySpecification {
            types: TypeConstraint::Selector(&selector),
            ids: &[],
            id_pattern: None,
        }];
        let speed = [name("speed")];
        let asked = RegistrationMatchSpecification {
            entities: &entities,
            attributes: &speed,
        };
        let nothing_asked = RegistrationMatchSpecification {
            entities: &entities,
            attributes: &[],
        };

        let entity_only = info(json!({"entities": [{"type": "Vehicle"}]}));
        assert!(asked.matches_information(&entity_only), "only entities registered: any attribute may be held");
        let declares_speed = info(json!({"entities": [{"type": "Vehicle"}], "relationshipNames": ["speed"]}));
        assert!(asked.matches_information(&declares_speed), "a relationship name counts");
        let declares_other = info(json!({"entities": [{"type": "Vehicle"}], "propertyNames": ["brandName"]}));
        assert!(!asked.matches_information(&declares_other));
        assert!(nothing_asked.matches_information(&declares_other), "no names asked for means every attribute");
    }

    /// The single attribute a request path names is held to each block on its
    /// own, beside the entity the path names.
    #[test]
    fn a_single_requested_attribute_matches_a_block_declaring_it_or_declaring_none() {
        let id = uri("urn:ngsi-ld:Vehicle:1");
        let entities = [EntitySpecification::of_entity(&[], &id)];
        let speed = name("https://example.org/speed");
        let specification = RegistrationMatchSpecification {
            entities: &entities,
            attributes: slice::from_ref(&speed),
        };
        assert!(specification.covers(&registration(&json!([
            {"entities": [{"type": "https://example.org/Vehicle"}], "propertyNames": ["https://example.org/speed"]}
        ]))));
        assert!(specification.covers(&registration(&json!([{"entities": [{"type": "https://example.org/Vehicle"}]}]))));
        assert!(!specification.covers(&registration(&json!([
            {"entities": [{"type": "https://example.org/Vehicle"}], "propertyNames": ["https://example.org/brandName"]}
        ]))));
        assert!(
            !specification.covers(&registration(&json!([
                {"entities": [{"type": "https://example.org/Vehicle", "id": "urn:ngsi-ld:Vehicle:2"}]},
                {"propertyNames": ["https://example.org/brandName"]}
            ]))),
            "the entity and the attribute must match on the same block"
        );
    }

    /// Clause 5.12: both axes hold on the same element, and the registration
    /// is trimmed to the elements on which they do.
    #[test]
    fn matching_information_keeps_only_elements_matching_on_both_axes() {
        let selector = type_selector("Vehicle");
        let entities = [EntitySpecification {
            types: TypeConstraint::Selector(&selector),
            ids: &[],
            id_pattern: None,
        }];
        let speed = [name("speed")];
        let specification = RegistrationMatchSpecification {
            entities: &entities,
            attributes: &speed,
        };
        let information = [
            info(json!({"entities": [{"type": "Vehicle"}], "propertyNames": ["speed"]})),
            info(json!({"entities": [{"type": "Building"}], "propertyNames": ["speed"]})),
            info(json!({"entities": [{"type": "Vehicle"}], "propertyNames": ["brandName"]})),
        ];
        let kept: Vec<usize> = specification
            .matching_information(&information)
            .map(|element| {
                information
                    .iter()
                    .position(|candidate| ptr::eq(candidate, element))
                    .expect("element of the slice")
            })
            .collect();
        pretty_assertions::assert_eq!(kept, vec![0]);
    }

    /// Table 5.2.10-1 forbids an empty `entities` array, so a block the engine
    /// reads never carries one.
    #[test]
    fn an_empty_entities_array_never_reads_as_a_registration_info() {
        assert!(serde_json::from_value::<RegistrationInfo>(json!({"entities": []})).is_err());
        assert!(serde_json::from_value::<RegistrationInfo>(json!({"entities": [], "propertyNames": ["speed"]})).is_err());
    }
}
