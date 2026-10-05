//! Clause 4.5.5.2, "Processing of Conflicting Transient Entities".
//!
//! Several Context Sources can hold the same Entity and disagree about when it
//! ceases to exist. Clause 4.5.5.3 resolves that disagreement destructively:
//! "If an `expiresAt` attribute is missing from at least one version of the
//! Entity received from registered Context Sources, the `expiresAt` attribute
//! is removed from the Entity." Clause 4.5.5.2 is what stops the transience a
//! source did report from being lost with it: before the versions are folded
//! together, each version that carries an entity-level `expiresAt` has that
//! instant pushed down onto its own Attributes, so the data that source
//! contributed still stops being served when the source said it would.
//!
//! The pass therefore has to run while the versions are still separate. Once
//! `merge_entity_attributes` has folded a version in, the entity-level value
//! it left behind is the reconciled one and no longer belongs to any single
//! source.

use crate::{attribute_instance_merge::MergePolicy, entity_result::policy_of, outcome::ForwardedEntities};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity::Entity;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Where one version of an Entity sits while the versions are still separate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VersionPosition {
    /// The version the merge is building on: the copy the broker holds itself,
    /// at this offset of the base slice.
    Held(usize),
    /// A version a context source returned, at this answer and offset.
    Received {
        /// Which contacted registration's answer holds it.
        answer: usize,
        /// Where in that answer's Entity Array it sits.
        offset: usize,
    },
}

/// What the versions of one Entity seen so far report for the entity-level
/// `expiresAt`.
///
/// Clause 4.5.5.2 applies only when the value "is not duplicated across all
/// versions of the Entity received across all registered Context Sources", so
/// the only distinction that matters is whether every version reported the
/// same thing -- a version reporting nothing included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpiryAgreement {
    /// Every version seen so far reported this same value.
    Unanimous(Option<DateTime<Utc>>),
    /// Two versions reported different values, so the mechanism applies.
    Divided,
}

impl ExpiryAgreement {
    /// Fold one more version's entity-level `expiresAt` into the agreement.
    fn observing(self, reported: Option<DateTime<Utc>>) -> ExpiryAgreement {
        match self {
            ExpiryAgreement::Unanimous(seen) if seen == reported => ExpiryAgreement::Unanimous(seen),
            ExpiryAgreement::Unanimous(_) | ExpiryAgreement::Divided => ExpiryAgreement::Divided,
        }
    }
}

/// Differentiate the conflicting transient versions of the Entities about to
/// be merged (clause 4.5.5.2).
///
/// `held` is what the merge builds on -- the copy this broker holds itself, or
/// on a retrieval the single entity being assembled -- and `received` is what
/// the contacted registrations answered. Both are taken mutably because the
/// clause rewrites each version in place, before any of them is folded.
pub(crate) fn differentiate_transient_versions(held: &mut [Entity], received: &mut [ForwardedEntities]) {
    for position in conflicting_versions(held, received) {
        let version = match position {
            VersionPosition::Held(offset) => held.get_mut(offset),
            VersionPosition::Received { answer, offset } => received.get_mut(answer).and_then(|answer| answer.entities.get_mut(offset)),
        };
        if let Some(version) = version {
            constrain_attributes_to_entity_expiry(version);
        }
    }
}

/// Every version that is a party to the clause 4.5.5.2 conflict, paired with
/// where it sits.
///
/// Clause 4.3.6.2 keeps an auxiliary registration out: its data "is only
/// included if it is supplementary to the context data otherwise available to
/// the Context Broker" and it "never overrides data held directly within a
/// Context Broker", so an auxiliary version can neither create the conflict
/// nor be differentiated by it.
fn parties<'a>(held: &'a [Entity], received: &'a [ForwardedEntities]) -> impl Iterator<Item = (VersionPosition, &'a Entity)> {
    let base = held.iter().enumerate().map(|(offset, version)| (VersionPosition::Held(offset), version));
    let remote = received
        .iter()
        .enumerate()
        .filter(|(_, answer)| matches!(policy_of(answer.mode), MergePolicy::Instances))
        .flat_map(|(answer, entities)| {
            entities
                .entities
                .iter()
                .enumerate()
                .map(move |(offset, version)| (VersionPosition::Received { answer, offset }, version))
        });
    base.chain(remote)
}

/// The versions carrying an entity-level `expiresAt` that their peers do not
/// duplicate, which are exactly the ones clause 4.5.5.2 differentiates.
fn conflicting_versions(held: &[Entity], received: &[ForwardedEntities]) -> Vec<VersionPosition> {
    let mut agreement: HashMap<&Identifier, ExpiryAgreement> = HashMap::new();
    for (_, version) in parties(held, received) {
        let folded = agreement
            .get(&version.id)
            .map_or(ExpiryAgreement::Unanimous(version.expires_at), |seen| seen.observing(version.expires_at));
        agreement.insert(&version.id, folded);
    }
    parties(held, received)
        .filter(|(_, version)| version.expires_at.is_some() && agreement.get(&version.id) == Some(&ExpiryAgreement::Divided))
        .map(|(position, _)| position)
        .collect()
}

/// Give every Attribute of one version an `expiresAt` no later than that
/// version's own entity-level one (clause 4.5.5.2).
///
/// "If no `expiresAt` attribute is present as a property of an Attribute, a
/// non-reified `expiresAt` attribute is added as a property of that Attribute
/// corresponding to the `expiresAt` found on the Entity", and "if the value on
/// the Attribute is further in the future than the `expiresAt` value found on
/// the Entity, the value of the `expiresAt` on the Attribute is overwritten to
/// corresponding to the earlier `DateTime`". An Attribute that already expires
/// no later than the Entity is left exactly as the source sent it.
///
/// `location`, `observationSpace` and `operationSpace` are Attributes of the
/// Entity (clause 5.2.4) and are constrained with the rest; a member the
/// broker carries through verbatim holds no attribute instance at all and is
/// untouched.
fn constrain_attributes_to_entity_expiry(version: &mut Entity) {
    let Some(entity_expiry) = version.expires_at else {
        return;
    };
    let members = version
        .attrs
        .values_mut()
        .chain(version.location.iter_mut())
        .chain(version.observation_space.iter_mut())
        .chain(version.operation_space.iter_mut());
    for member in members {
        for instance in member.instances_mut() {
            let common = instance.common_mut();
            common.expires_at = Some(common.expires_at.map_or(entity_expiry, |attribute_expiry| attribute_expiry.min(entity_expiry)));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{outcome::ForwardedEntities, transient_entity_conflict::differentiate_transient_versions};
    use andromeda_ngsi_ld_model::{
        attribute::instance::{Attribute, AttributeValue},
        entity::Entity,
        registration::forwarding_mode::ForwardingMode,
    };
    use serde_json::{Value, json};

    fn entity(document: &Value) -> Entity {
        serde_json::from_value(document.clone()).expect("valid entity fixture") // clone: the fixture is reused across assertions
    }

    fn answer(mode: ForwardingMode, entities: Vec<Entity>) -> ForwardedEntities {
        ForwardedEntities {
            registration_id: Some("urn:ngsi-ld:Registration:1".parse().expect("registration URI")),
            mode,
            entities,
            warnings: Vec::new(),
            linked_map: None,
        }
    }

    fn attribute_expiry(version: &Entity, name: &str) -> Option<String> {
        version
            .attrs
            .get(name)
            .map(AttributeValue::instances)
            .into_iter()
            .flatten()
            .next()
            .and_then(Attribute::expires_at)
            .map(|at| at.to_rfc3339())
    }

    /// Clause 4.5.5.2: a version marked with an entity-level `expiresAt` that
    /// its peer does not duplicate has that instant added to every Attribute
    /// of its own that carries none.
    #[test]
    fn an_entity_expiry_no_peer_duplicates_is_added_to_the_attributes_that_lack_one() {
        let mut held = Vec::new();
        let mut received = vec![
            answer(
                ForwardingMode::Inclusive,
                vec![entity(&json!({
                    "id": "urn:ngsi-ld:Vehicle:1",
                    "type": "Vehicle",
                    "expiresAt": "2030-01-01T00:00:00Z",
                    "speed": {"type": "Property", "value": 10}
                }))],
            ),
            answer(
                ForwardingMode::Inclusive,
                vec![entity(&json!({
                    "id": "urn:ngsi-ld:Vehicle:1",
                    "type": "Vehicle",
                    "fuel": {"type": "Property", "value": 70}
                }))],
            ),
        ];

        differentiate_transient_versions(&mut held, &mut received);

        pretty_assertions::assert_eq!(
            attribute_expiry(&received[0].entities[0], "speed"),
            Some("2030-01-01T00:00:00+00:00".to_string())
        );
        pretty_assertions::assert_eq!(attribute_expiry(&received[1].entities[0], "fuel"), None);
    }

    /// Clause 4.5.5.2: an Attribute whose own `expiresAt` is further in the
    /// future than the Entity's is overwritten with the earlier `DateTime`; one
    /// that already expires earlier is left alone.
    #[test]
    fn an_attribute_expiry_later_than_the_entity_is_clamped_and_an_earlier_one_is_kept() {
        let mut held = Vec::new();
        let mut received = vec![
            answer(
                ForwardingMode::Inclusive,
                vec![entity(&json!({
                    "id": "urn:ngsi-ld:Vehicle:1",
                    "type": "Vehicle",
                    "expiresAt": "2030-01-01T00:00:00Z",
                    "speed": {"type": "Property", "value": 10, "expiresAt": "2031-01-01T00:00:00Z"},
                    "fuel": {"type": "Property", "value": 70, "expiresAt": "2029-01-01T00:00:00Z"}
                }))],
            ),
            answer(
                ForwardingMode::Inclusive,
                vec![entity(&json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"}))],
            ),
        ];

        differentiate_transient_versions(&mut held, &mut received);

        pretty_assertions::assert_eq!(
            attribute_expiry(&received[0].entities[0], "speed"),
            Some("2030-01-01T00:00:00+00:00".to_string())
        );
        pretty_assertions::assert_eq!(
            attribute_expiry(&received[0].entities[0], "fuel"),
            Some("2029-01-01T00:00:00+00:00".to_string())
        );
    }

    /// Clause 4.5.5.2 applies only to an `expiresAt` that "is not duplicated
    /// across all versions": versions that agree are left exactly as they are,
    /// and clause 4.5.5.3 then keeps the entity-level value.
    #[test]
    fn versions_that_agree_on_the_entity_expiry_are_left_untouched() {
        let mut held = Vec::new();
        let marked = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "expiresAt": "2030-01-01T00:00:00Z",
            "speed": {"type": "Property", "value": 10}
        });
        let mut received = vec![
            answer(ForwardingMode::Inclusive, vec![entity(&marked)]),
            answer(ForwardingMode::Inclusive, vec![entity(&marked)]),
        ];

        differentiate_transient_versions(&mut held, &mut received);

        pretty_assertions::assert_eq!(attribute_expiry(&received[0].entities[0], "speed"), None);
        pretty_assertions::assert_eq!(attribute_expiry(&received[1].entities[0], "speed"), None);
    }

    /// A lone version is duplicated across all versions there are, so nothing
    /// is differentiated and the entity-level value survives clause 4.5.5.3.
    #[test]
    fn a_single_version_is_never_in_conflict_with_itself() {
        let mut held = Vec::new();
        let mut received = vec![answer(
            ForwardingMode::Inclusive,
            vec![entity(&json!({
                "id": "urn:ngsi-ld:Vehicle:1",
                "type": "Vehicle",
                "expiresAt": "2030-01-01T00:00:00Z",
                "speed": {"type": "Property", "value": 10}
            }))],
        )];

        differentiate_transient_versions(&mut held, &mut received);

        pretty_assertions::assert_eq!(attribute_expiry(&received[0].entities[0], "speed"), None);
    }

    /// Clause 4.5.5.3 takes the furthest-in-the-future entity-level value when
    /// every version carries one, so two versions carrying different instants
    /// are still in conflict and each keeps its own instant per Attribute.
    #[test]
    fn versions_carrying_different_instants_each_keep_their_own_on_their_attributes() {
        let mut held = Vec::new();
        let mut received = vec![
            answer(
                ForwardingMode::Inclusive,
                vec![entity(&json!({
                    "id": "urn:ngsi-ld:Vehicle:1",
                    "type": "Vehicle",
                    "expiresAt": "2030-01-01T00:00:00Z",
                    "speed": {"type": "Property", "value": 10}
                }))],
            ),
            answer(
                ForwardingMode::Inclusive,
                vec![entity(&json!({
                    "id": "urn:ngsi-ld:Vehicle:1",
                    "type": "Vehicle",
                    "expiresAt": "2040-01-01T00:00:00Z",
                    "fuel": {"type": "Property", "value": 70}
                }))],
            ),
        ];

        differentiate_transient_versions(&mut held, &mut received);

        pretty_assertions::assert_eq!(
            attribute_expiry(&received[0].entities[0], "speed"),
            Some("2030-01-01T00:00:00+00:00".to_string())
        );
        pretty_assertions::assert_eq!(
            attribute_expiry(&received[1].entities[0], "fuel"),
            Some("2040-01-01T00:00:00+00:00".to_string())
        );
    }

    /// Clause 4.3.6.2: an auxiliary version is not a party to the conflict, so
    /// it neither triggers the mechanism nor is differentiated by it.
    #[test]
    fn an_auxiliary_version_neither_triggers_nor_receives_the_differentiation() {
        let mut held = Vec::new();
        let mut received = vec![
            answer(
                ForwardingMode::Inclusive,
                vec![entity(&json!({
                    "id": "urn:ngsi-ld:Vehicle:1",
                    "type": "Vehicle",
                    "expiresAt": "2030-01-01T00:00:00Z",
                    "speed": {"type": "Property", "value": 10}
                }))],
            ),
            answer(
                ForwardingMode::Auxiliary,
                vec![entity(&json!({
                    "id": "urn:ngsi-ld:Vehicle:1",
                    "type": "Vehicle",
                    "expiresAt": "2040-01-01T00:00:00Z",
                    "fuel": {"type": "Property", "value": 70}
                }))],
            ),
        ];

        differentiate_transient_versions(&mut held, &mut received);

        pretty_assertions::assert_eq!(attribute_expiry(&received[0].entities[0], "speed"), None);
        pretty_assertions::assert_eq!(attribute_expiry(&received[1].entities[0], "fuel"), None);
    }

    /// The copy the broker holds itself is one of the representations clauses
    /// 5.7.1.4 and 5.7.2.4 merge, so it counts as a version both for the
    /// trigger and for the differentiation.
    #[test]
    fn the_locally_held_version_is_a_party_to_the_conflict() {
        let mut held = vec![entity(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "expiresAt": "2030-01-01T00:00:00Z",
            "speed": {"type": "Property", "value": 10}
        }))];
        let mut received = vec![answer(
            ForwardingMode::Inclusive,
            vec![entity(
                &json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "fuel": {"type": "Property", "value": 70}}),
            )],
        )];

        differentiate_transient_versions(&mut held, &mut received);

        pretty_assertions::assert_eq!(attribute_expiry(&held[0], "speed"), Some("2030-01-01T00:00:00+00:00".to_string()));
        pretty_assertions::assert_eq!(attribute_expiry(&received[0].entities[0], "fuel"), None);
    }

    /// The named `GeoProperties` are Attributes of the Entity (clause 5.2.4)
    /// and take the propagation with the rest; a member carried through
    /// verbatim holds no instance and survives untouched.
    #[test]
    fn the_named_geo_properties_take_the_propagation_and_a_verbatim_member_is_skipped() {
        let mut held = Vec::new();
        let mut received = vec![
            answer(
                ForwardingMode::Inclusive,
                vec![entity(&json!({
                    "id": "urn:ngsi-ld:Vehicle:1",
                    "type": "Vehicle",
                    "expiresAt": "2030-01-01T00:00:00Z",
                    "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}},
                    "observationSpace": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [3.0, 4.0]}},
                    "operationSpace": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [5.0, 6.0]}},
                    "unstructured": "carried through verbatim"
                }))],
            ),
            answer(
                ForwardingMode::Inclusive,
                vec![entity(&json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"}))],
            ),
        ];

        differentiate_transient_versions(&mut held, &mut received);

        let version = &received[0].entities[0];
        for member in [&version.location, &version.observation_space, &version.operation_space] {
            let expiry = member
                .as_ref()
                .map(AttributeValue::instances)
                .into_iter()
                .flatten()
                .next()
                .and_then(Attribute::expires_at)
                .map(|at| at.to_rfc3339());
            pretty_assertions::assert_eq!(expiry, Some("2030-01-01T00:00:00+00:00".to_string()));
        }
        assert!(matches!(version.attrs.get("unstructured"), Some(AttributeValue::Single(_))));
    }

    /// Every instance of a multi-attribute is an Attribute of the Entity, so
    /// each is constrained on its own terms.
    #[test]
    fn every_instance_of_a_multi_attribute_is_constrained_independently() {
        let mut held = Vec::new();
        let mut received = vec![
            answer(
                ForwardingMode::Inclusive,
                vec![entity(&json!({
                    "id": "urn:ngsi-ld:Vehicle:1",
                    "type": "Vehicle",
                    "expiresAt": "2030-01-01T00:00:00Z",
                    "speed": [
                        {"type": "Property", "value": 10, "datasetId": "urn:ngsi-ld:ds:1", "expiresAt": "2031-01-01T00:00:00Z"},
                        {"type": "Property", "value": 20, "datasetId": "urn:ngsi-ld:ds:2"}
                    ]
                }))],
            ),
            answer(
                ForwardingMode::Inclusive,
                vec![entity(&json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"}))],
            ),
        ];

        differentiate_transient_versions(&mut held, &mut received);

        let expiries: Vec<Option<String>> = received[0].entities[0]
            .attrs
            .get("speed")
            .map(AttributeValue::instances)
            .into_iter()
            .flatten()
            .map(|instance| instance.expires_at().map(|at| at.to_rfc3339()))
            .collect();
        pretty_assertions::assert_eq!(
            expiries,
            vec![Some("2030-01-01T00:00:00+00:00".to_string()), Some("2030-01-01T00:00:00+00:00".to_string())]
        );
    }

    /// Versions of different Entities never form one population: a conflict
    /// about one Entity leaves another Entity's version alone.
    #[test]
    fn each_entity_forms_its_own_population_of_versions() {
        let mut held = Vec::new();
        let mut received = vec![
            answer(
                ForwardingMode::Inclusive,
                vec![
                    entity(&json!({
                        "id": "urn:ngsi-ld:Vehicle:1",
                        "type": "Vehicle",
                        "expiresAt": "2030-01-01T00:00:00Z",
                        "speed": {"type": "Property", "value": 10}
                    })),
                    entity(&json!({
                        "id": "urn:ngsi-ld:Vehicle:2",
                        "type": "Vehicle",
                        "expiresAt": "2030-01-01T00:00:00Z",
                        "speed": {"type": "Property", "value": 20}
                    })),
                ],
            ),
            answer(
                ForwardingMode::Inclusive,
                vec![entity(
                    &json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "fuel": {"type": "Property", "value": 70}}),
                )],
            ),
        ];

        differentiate_transient_versions(&mut held, &mut received);

        pretty_assertions::assert_eq!(
            attribute_expiry(&received[0].entities[0], "speed"),
            Some("2030-01-01T00:00:00+00:00".to_string())
        );
        pretty_assertions::assert_eq!(attribute_expiry(&received[0].entities[1], "speed"), None);
    }
}
