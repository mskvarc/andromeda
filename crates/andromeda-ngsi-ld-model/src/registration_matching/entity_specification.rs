//! One entity specification of clause 5.12 and its match against one
//! registration block's entity axis.

use crate::{
    entity_info::{EntityInfo, EntityTypeSelection},
    registration::info::RegistrationInfo,
    subscription::entity_selector::EntitySelector,
};
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_query::{identifier_axis::IdentifierAxis, pattern::Pattern, type_selection::grammar::TypeExprRaw};
use std::slice;

/// What an entity specification requires of the types a registration names.
#[derive(Debug, Clone, Copy)]
pub enum TypeConstraint<'spec> {
    /// No type is required: an identifier-only request, or an entity whose
    /// types are not known.
    Any,
    /// The clause 4.17 selector the registered types must satisfy.
    Selector(&'spec TypeExprRaw),
    /// The types of one entity: the registered types must name at least one of
    /// them, which is the clause 4.17 alternation of the list.
    OneOf(&'spec [EntityTypeName]),
}

impl TypeConstraint<'_> {
    /// Whether the types an `EntityInfo` registers satisfy the constraint.
    fn admits(&self, registered: &EntityTypeSelection) -> bool {
        match self {
            TypeConstraint::Any => true,
            TypeConstraint::Selector(selector) => selector.matches(registered.names()),
            TypeConstraint::OneOf(types) => types.iter().any(|entity_type| registered.names().contains(entity_type)),
        }
    }
}

/// One entity specification of clause 5.12: a type constraint, entity
/// identifiers and an identifier pattern, each optional.
///
/// A discovery query yields one of these from its `type`, `id` and
/// `idPattern`; a subscription yields one per `EntitySelector`; a concrete
/// entity yields one from its types and identifier.
#[derive(Debug, Clone, Copy)]
pub struct EntitySpecification<'spec> {
    /// What the registered types must satisfy.
    pub types: TypeConstraint<'spec>,
    /// The entity identifiers asked for; empty when none were.
    pub ids: &'spec [Identifier],
    /// The identifier pattern asked for, if any.
    pub id_pattern: Option<&'spec Pattern>,
}

impl<'spec> EntitySpecification<'spec> {
    /// The specification a subscription's `EntitySelector` (clause 5.2.33) is.
    #[must_use]
    pub const fn from_selector(selector: &'spec EntitySelector) -> EntitySpecification<'spec> {
        EntitySpecification {
            types: TypeConstraint::Selector(&selector.type_),
            ids: selector.id.as_slice(),
            id_pattern: selector.id_pattern.as_ref(),
        }
    }

    /// The specification one concrete entity is: any of its `types`, and
    /// exactly its `id`.
    ///
    /// An entity whose types are not known passes no type, and is then matched
    /// on its identifier alone.
    #[must_use]
    pub const fn of_entity(types: &'spec [EntityTypeName], id: &'spec Identifier) -> EntitySpecification<'spec> {
        EntitySpecification {
            types: if types.is_empty() {
                TypeConstraint::Any
            } else {
                TypeConstraint::OneOf(types)
            },
            ids: slice::from_ref(id),
            id_pattern: None,
        }
    }

    /// What this specification constrains about the identifier, with the
    /// clause 5.2.33 precedence of `id` over `idPattern` applied.
    #[must_use]
    pub const fn identifier_axis(&self) -> IdentifierAxis<'spec> {
        IdentifierAxis::of(self.ids, self.id_pattern)
    }

    /// Whether this specification matches the entity axis of one
    /// `RegistrationInfo` (clause 5.12): a block registering no entity is
    /// matching, otherwise one of its `EntityInfo` must match.
    #[must_use]
    pub fn matches_entity_axis(&self, block: &RegistrationInfo) -> bool {
        block
            .entities
            .as_deref()
            .is_none_or(|entities| entities.iter().any(|entity| self.matches_entity_info(entity)))
    }

    /// Whether this specification matches one `EntityInfo` (clause 5.12): the
    /// type constraint holds on the registered type(s), and one of the five
    /// identifier conditions holds.
    fn matches_entity_info(&self, info: &EntityInfo) -> bool {
        self.types.admits(&info.type_) && self.identifier_axis_matches(info)
    }

    /// The five clause 5.12 identifier conditions, read with the clause 5.2.8
    /// precedence of a registered `id` over a registered `idPattern`.
    fn identifier_axis_matches(&self, info: &EntityInfo) -> bool {
        let axis = self.identifier_axis();
        match (&info.id, &info.id_pattern, axis) {
            // Nothing specified; or the EntityInfo contains neither an id nor
            // an idPattern; or both sides carry a pattern, which is taken as
            // matching since whether two patterns admit a common identifier is
            // not decidable cheaply.
            (_, _, IdentifierAxis::Any) | (None, None, _) | (None, Some(_), IdentifierAxis::Pattern(_)) => true,
            // The specified identifiers or pattern admit the registered id.
            (Some(registered), _, axis) => axis.admits(registered),
            // One of the specified identifiers matches the registered pattern.
            (None, Some(registered), IdentifierAxis::Ids(ids)) => ids.iter().any(|id| registered.is_match(id.as_str())),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_info::EntityInfo,
        registration::info::RegistrationInfo,
        registration_matching::entity_specification::{EntitySpecification, TypeConstraint},
        subscription::entity_selector::EntitySelector,
    };
    use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
    use andromeda_ngsi_ld_query::{pattern::Pattern, type_selection::grammar::TypeExprRaw};
    use serde_json::json;

    fn uri(value: &str) -> Identifier {
        value.parse().expect("valid URI")
    }

    fn type_name(value: &str) -> EntityTypeName {
        value.parse().expect("valid type name")
    }

    fn type_selector(raw: &str) -> TypeExprRaw {
        raw.parse().expect("valid type selector")
    }

    fn pattern(raw: &str) -> Pattern {
        Pattern::parse(raw).expect("valid pattern")
    }

    fn entity_info(value: serde_json::Value) -> EntityInfo {
        serde_json::from_value(value).expect("EntityInfo parses")
    }

    fn block(value: serde_json::Value) -> RegistrationInfo {
        serde_json::from_value(value).expect("information parses")
    }

    /// The five identifier conditions of clause 5.12, with the clause 5.2.8
    /// precedence of `id` over `idPattern`.
    #[test]
    fn the_identifier_conditions_of_clause_5_12() {
        let selector = type_selector("Vehicle");
        let ids = [uri("urn:ngsi-ld:Vehicle:1")];
        let fleet = pattern("urn:ngsi-ld:Fleet.*");
        let by_id = EntitySpecification {
            types: TypeConstraint::Selector(&selector),
            ids: &ids,
            id_pattern: None,
        };
        let by_pattern = EntitySpecification {
            types: TypeConstraint::Selector(&selector),
            ids: &[],
            id_pattern: Some(&fleet),
        };

        let unconstrained = entity_info(json!({"type": "Vehicle"}));
        assert!(by_id.matches_entity_info(&unconstrained), "neither id nor idPattern matches any identifier");
        assert!(by_pattern.matches_entity_info(&unconstrained));

        let exact = entity_info(json!({"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:1"}));
        assert!(by_id.matches_entity_info(&exact), "a specified identifier equals the registered id");
        assert!(
            !by_pattern.matches_entity_info(&exact),
            "the specified pattern does not match the registered id"
        );

        let fleet_member = entity_info(json!({"type": "Vehicle", "id": "urn:ngsi-ld:Fleet:7"}));
        assert!(by_pattern.matches_entity_info(&fleet_member), "the specified pattern matches the registered id");
        assert!(!by_id.matches_entity_info(&fleet_member));

        let registered_pattern = entity_info(json!({"type": "Vehicle", "idPattern": "urn:ngsi-ld:Vehicle:.*"}));
        assert!(
            by_id.matches_entity_info(&registered_pattern),
            "a specified identifier matches the registered pattern"
        );
        assert!(
            by_pattern.matches_entity_info(&registered_pattern),
            "both patterns present is taken as matching"
        );

        let both = entity_info(json!({"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:9", "idPattern": "urn:ngsi-ld:Vehicle:.*"}));
        assert!(
            !by_id.matches_entity_info(&both),
            "a registered id takes precedence over the registered pattern"
        );
    }

    /// Clause 5.2.8: the registered `id` decides, so the pattern beside it does
    /// not widen what the entry covers.
    #[test]
    fn a_concrete_entity_is_covered_by_a_registered_id_beside_a_pattern_only_through_that_id() {
        let element = block(json!({"entities": [{"type": "https://example.org/Vehicle", "id": "urn:ngsi-ld:Vehicle:7", "idPattern": ".*"}]}));
        let seven = uri("urn:ngsi-ld:Vehicle:7");
        let one = uri("urn:ngsi-ld:Vehicle:1");
        assert!(EntitySpecification::of_entity(&[], &seven).matches_entity_axis(&element));
        assert!(!EntitySpecification::of_entity(&[], &one).matches_entity_axis(&element));
    }

    #[test]
    fn a_concrete_entity_is_covered_by_a_registered_pattern_alone_when_it_matches() {
        let element = block(json!({"entities": [{"type": "https://example.org/Vehicle", "idPattern": "urn:ngsi-ld:Vehicle:.*"}]}));
        assert!(EntitySpecification::of_entity(&[], &uri("urn:ngsi-ld:Vehicle:2")).matches_entity_axis(&element));
        assert!(!EntitySpecification::of_entity(&[], &uri("urn:ngsi-ld:Building:2")).matches_entity_axis(&element));
        let unconstrained = block(json!({"entities": [{"type": "https://example.org/Vehicle"}]}));
        assert!(EntitySpecification::of_entity(&[], &uri("urn:ngsi-ld:Building:2")).matches_entity_axis(&unconstrained));
    }

    /// The read and purge-by-id paths pass no type on purpose, so a
    /// registration scoped to a type still covers an id-only request.
    #[test]
    fn a_concrete_entity_with_no_resolved_type_is_matched_on_its_identifier_alone() {
        let element = block(json!({"entities": [{"type": "https://example.org/Vehicle"}]}));
        let id = uri("urn:ngsi-ld:Vehicle:1");
        let specification = EntitySpecification::of_entity(&[], &id);
        assert!(matches!(specification.types, TypeConstraint::Any));
        assert!(specification.matches_entity_axis(&element));
    }

    /// Clause 5.12 holds the type selector against the entity's types (plural):
    /// a registration covering one of an entity's several types matches it,
    /// wherever that type stands among them, and one covering none does not.
    #[test]
    fn one_of_the_entity_types_admitted_by_the_registered_types_matches() {
        let device = type_name("https://example.org/Device");
        let vehicle = type_name("https://example.org/Vehicle");
        let building = type_name("https://example.org/Building");
        let id = uri("urn:ngsi-ld:Vehicle:1");
        let vehicles = block(json!({"entities": [{"type": "https://example.org/Vehicle"}]}));
        let devices = block(json!({"entities": [{"type": "https://example.org/Device"}]}));
        let buildings = block(json!({"entities": [{"type": "https://example.org/Building"}]}));
        let multi_type = block(json!({"entities": [{"type": ["https://example.org/Vehicle", "https://example.org/Device"]}]}));

        let device_and_vehicle = [device.clone(), vehicle.clone()];
        let vehicle_and_device = [vehicle, device.clone()];
        assert!(EntitySpecification::of_entity(&device_and_vehicle, &id).matches_entity_axis(&vehicles));
        assert!(EntitySpecification::of_entity(&vehicle_and_device, &id).matches_entity_axis(&vehicles));
        assert!(!EntitySpecification::of_entity(&device_and_vehicle, &id).matches_entity_axis(&buildings));

        let only_building = [building];
        assert!(
            !EntitySpecification::of_entity(&only_building, &id).matches_entity_axis(&devices),
            "a differing known entity type does not match a type-scoped block"
        );
        let only_device = [device];
        assert!(EntitySpecification::of_entity(&only_device, &id).matches_entity_axis(&multi_type));
        assert!(!EntitySpecification::of_entity(&only_building, &id).matches_entity_axis(&multi_type));
    }

    #[test]
    fn a_block_registering_no_entity_matches_any_entity_specification() {
        let element = block(json!({"propertyNames": ["speed"]}));
        let selector = type_selector("Vehicle");
        let specification = EntitySpecification {
            types: TypeConstraint::Selector(&selector),
            ids: &[],
            id_pattern: None,
        };
        assert!(specification.matches_entity_axis(&element));
    }

    #[test]
    fn the_type_selector_must_match_one_of_the_registered_types() {
        let several = block(json!({"entities": [{"type": ["Vehicle", "Car"]}]}));
        let type_only = |raw: &str| {
            let selector = type_selector(raw);
            EntitySpecification {
                types: TypeConstraint::Selector(&selector),
                ids: &[],
                id_pattern: None,
            }
            .matches_entity_axis(&several)
        };
        assert!(type_only("Car"));
        assert!(type_only("Vehicle"));
        assert!(!type_only("Building"));
        assert!(type_only("Building|Car"));
    }

    #[test]
    fn a_subscription_selector_becomes_one_specification() {
        let selector: EntitySelector = serde_json::from_value(json!({"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:1"})).expect("parses");
        let specification = EntitySpecification::from_selector(&selector);
        pretty_assertions::assert_eq!(specification.ids.len(), 1);
        assert!(specification.id_pattern.is_none());
        assert!(matches!(specification.types, TypeConstraint::Selector(_)));
    }
}
