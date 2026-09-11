//! Matching a specification of entities and attributes against a registration's
//! `information` (ETSI GS CIM 009 clause 5.12).
//!
//! Discovery (clause 5.10.2) and context source registration subscriptions
//! (clause 5.11.2) both ask which `RegistrationInfo` elements match the
//! entities and attributes they specify; clause 5.12 gives the one reading both
//! use, and clauses 5.10.2.5 and 5.11.7 return the registration trimmed to the
//! matching elements. This module is that reading.

use crate::{entity_info::EntityInfo, registration::info::RegistrationInfo, subscription::EntitySelector};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_query::{pattern::Pattern, type_selection::grammar::TypeExprRaw};

/// One entity specification of clause 5.12: a type selector, entity
/// identifiers and an identifier pattern, each optional.
///
/// A discovery query yields one of these from its `type`, `id` and
/// `idPattern`; a subscription yields one per `EntitySelector`.
#[derive(Debug, Clone, Copy)]
pub struct EntitySpecification<'spec> {
    /// The clause 4.17 selector the registered types must satisfy.
    pub type_selector: Option<&'spec TypeExprRaw>,
    /// The entity identifiers asked for; empty when none were.
    pub ids: &'spec [Uri],
    /// The identifier pattern asked for, if any.
    pub id_pattern: Option<&'spec Pattern>,
}

impl<'spec> EntitySpecification<'spec> {
    /// The specification a subscription's `EntitySelector` (clause 5.2.33) is.
    #[must_use]
    pub const fn from_selector(selector: &'spec EntitySelector) -> EntitySpecification<'spec> {
        EntitySpecification {
            type_selector: Some(&selector.type_),
            ids: selector.id.as_slice(),
            id_pattern: selector.id_pattern.as_ref(),
        }
    }

    /// Whether this specification matches one `EntityInfo` (clause 5.12): the
    /// type selector matches the registered type(s), and one of the five
    /// identifier conditions holds.
    #[must_use]
    pub fn matches_entity_info(&self, info: &EntityInfo) -> bool {
        let type_matches = self.type_selector.is_none_or(|selector| selector.matches(info.type_.names()));
        type_matches && self.identifier_axis_matches(info)
    }

    /// The five clause 5.12 identifier conditions, read with the clause 5.2.8
    /// precedence of a registered `id` over a registered `idPattern`.
    fn identifier_axis_matches(&self, info: &EntityInfo) -> bool {
        if self.ids.is_empty() && self.id_pattern.is_none() {
            return true;
        }
        match (&info.id, &info.id_pattern) {
            // The EntityInfo contains neither an id nor an idPattern.
            (None, None) => true,
            // One of the specified identifiers equals the registered id, or the
            // specified pattern matches it.
            (Some(registered), _) => self.ids.contains(registered) || self.id_pattern.is_some_and(|pattern| pattern.is_match(registered.as_str())),
            // One of the specified identifiers matches the registered pattern,
            // or both sides carry a pattern, which is taken as matching since
            // whether two patterns admit a common identifier is not decidable
            // cheaply.
            (None, Some(registered)) => self.ids.iter().any(|id| registered.is_match(id.as_str())) || self.id_pattern.is_some(),
        }
    }
}

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

    /// Clause 5.12: if there is no `EntityInfo` in the element, the entity
    /// specification is considered matching; otherwise some specification must
    /// match some `EntityInfo`.
    fn entity_axis_matches(&self, info: &RegistrationInfo) -> bool {
        let Some(entities) = info.entities.as_deref().filter(|entities| !entities.is_empty()) else {
            return true;
        };
        if self.entities.is_empty() {
            return true;
        }
        self.entities
            .iter()
            .any(|specification| entities.iter().any(|entity| specification.matches_entity_info(entity)))
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
mod tests {
    use super::*;
    use serde_json::json;
    use std::ptr;

    fn info(value: serde_json::Value) -> RegistrationInfo {
        serde_json::from_value(value).expect("information parses")
    }

    fn uri(value: &str) -> Uri {
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

    /// Whether a specification of one type selector and nothing else matches
    /// `element`.
    fn type_only_matches(raw: &str, element: &RegistrationInfo) -> bool {
        let selector = type_selector(raw);
        let entities = [EntitySpecification {
            type_selector: Some(&selector),
            ids: &[],
            id_pattern: None,
        }];
        RegistrationMatchSpecification {
            entities: &entities,
            attributes: &[],
        }
        .matches_information(element)
    }

    #[test]
    fn a_registration_info_without_entities_matches_any_entity_specification() {
        let element = info(json!({"propertyNames": ["speed"]}));
        assert!(type_only_matches("Vehicle", &element));
    }

    #[test]
    fn the_type_selector_must_match_one_of_the_registered_types() {
        let several = info(json!({"entities": [{"type": ["Vehicle", "Car"]}]}));
        assert!(type_only_matches("Car", &several));
        assert!(type_only_matches("Vehicle", &several));
        assert!(!type_only_matches("Building", &several));
        assert!(type_only_matches("(Building|Car)", &several));
    }

    /// The five identifier conditions of clause 5.12, with the clause 5.2.8
    /// precedence of `id` over `idPattern`.
    #[test]
    fn the_identifier_conditions_of_clause_5_12() {
        let selector = type_selector("Vehicle");
        let ids = [uri("urn:ngsi-ld:Vehicle:1")];
        let fleet = pattern("urn:ngsi-ld:Fleet.*");
        let by_id = EntitySpecification {
            type_selector: Some(&selector),
            ids: &ids,
            id_pattern: None,
        };
        let by_pattern = EntitySpecification {
            type_selector: Some(&selector),
            ids: &[],
            id_pattern: Some(&fleet),
        };

        let unconstrained: EntityInfo = serde_json::from_value(json!({"type": "Vehicle"})).expect("parses");
        assert!(by_id.matches_entity_info(&unconstrained), "neither id nor idPattern matches any identifier");
        assert!(by_pattern.matches_entity_info(&unconstrained));

        let exact: EntityInfo = serde_json::from_value(json!({"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:1"})).expect("parses");
        assert!(by_id.matches_entity_info(&exact), "a specified identifier equals the registered id");
        assert!(
            !by_pattern.matches_entity_info(&exact),
            "the specified pattern does not match the registered id"
        );

        let fleet_member: EntityInfo = serde_json::from_value(json!({"type": "Vehicle", "id": "urn:ngsi-ld:Fleet:7"})).expect("parses");
        assert!(by_pattern.matches_entity_info(&fleet_member), "the specified pattern matches the registered id");
        assert!(!by_id.matches_entity_info(&fleet_member));

        let registered_pattern: EntityInfo = serde_json::from_value(json!({"type": "Vehicle", "idPattern": "urn:ngsi-ld:Vehicle:.*"})).expect("parses");
        assert!(
            by_id.matches_entity_info(&registered_pattern),
            "a specified identifier matches the registered pattern"
        );
        assert!(
            by_pattern.matches_entity_info(&registered_pattern),
            "both patterns present is taken as matching"
        );

        let both: EntityInfo =
            serde_json::from_value(json!({"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:9", "idPattern": "urn:ngsi-ld:Vehicle:.*"})).expect("parses");
        assert!(
            !by_id.matches_entity_info(&both),
            "a registered id takes precedence over the registered pattern"
        );
    }

    #[test]
    fn attribute_names_match_when_none_asked_none_registered_or_one_shared() {
        let selector = type_selector("Vehicle");
        let entities = [EntitySpecification {
            type_selector: Some(&selector),
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

    /// Clause 5.12: both axes hold on the same element, and the registration
    /// is trimmed to the elements on which they do.
    #[test]
    fn matching_information_keeps_only_elements_matching_on_both_axes() {
        let selector = type_selector("Vehicle");
        let entities = [EntitySpecification {
            type_selector: Some(&selector),
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

    #[test]
    fn a_subscription_selector_becomes_one_specification() {
        let selector: EntitySelector = serde_json::from_value(json!({"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:1"})).expect("parses");
        let specification = EntitySpecification::from_selector(&selector);
        pretty_assertions::assert_eq!(specification.ids.len(), 1);
        assert!(specification.id_pattern.is_none());
        assert!(specification.type_selector.is_some());
    }
}
