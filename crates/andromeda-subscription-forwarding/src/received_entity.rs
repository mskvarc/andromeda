//! One entity of an inbound notification, read into the model under the
//! context it was written in (clause 5.8.6).
//!
//! The sender spelled its entity under the context of the copy it holds -- the
//! notification's own, or the `@context` an `application/ld+json` notification
//! writes on each entity -- so the entity is expanded once under that context
//! into the written document the local filters read and the local
//! subscription's compaction writes. A written document, unlike an entity
//! state, holds the deletions a source notifies. Clause 5.8.6 reports a
//! deleted attribute with "only the name of the attribute as key and the URI
//! "urn:ngsi-ld:null" as value" unless a `datasetId`, the system attributes or
//! a previous value must be reported with it, and then as an object whose
//! value member holds its family's deletion encoding (clause 4.5.0). The first
//! names no family and stays the deletion of the whole member, relayed as the
//! bare Null it arrived as; the second is the deletion of the instance of its
//! `datasetId`, relayed in the object form.
//!
//! Completing a split entity merges on the model: the deletions the sender
//! notified, of whole attributes and of single instances, are set aside, the
//! rest of its part becomes the version the local copy and every other holder
//! fold into (clause 4.5.5), and the deletions are put back on the completed
//! entity, which no other holder's instance overrides. The previous values the
//! sender reported under `showChanges` stay the sender's: no other holder saw
//! the change.

use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_entity::{
    attribute_instances::InstanceRemoval,
    document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument, written_value::ValueSlot},
    entity_state::EntityState,
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_expansion::{
    entity_expansion::EntityExpansion,
    error::ExpansionError,
    received_state::{ReceivedDocument, ReceivedRefusal, ReceivedState},
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::received::{ContextualEntity, ElementRefusal};
use indexmap::{IndexMap, map::Entry};
use thiserror::Error;

/// Why one entity of an inbound notification is not re-assembled.
#[derive(Debug, Error)]
pub(crate) enum ReceivedEntityRefusal {
    /// The entity's own `@context` does not resolve or process.
    #[error("its own @context cannot be processed: {0}")]
    OwnContext(#[source] ExpansionError),
    /// The entity, read under the context it was written in, is no entity the
    /// model can hold.
    #[error(transparent)]
    Undecodable(Box<ElementRefusal>),
    /// The entity does not expand under the context it was written in.
    #[error(transparent)]
    Expansion(#[from] ReceivedRefusal),
    /// The part the sender notified is no entity a holder's version can be
    /// merged onto.
    #[error("the notified part cannot be completed: {0}")]
    Incomplete(#[source] EntityRefusal),
}

/// Read one entity of an inbound notification under its own `@context` when
/// it names one (JSON-LD 1.1: a node's context applies to that node), else
/// under the notification's, `inbound`, its members read as the core terms
/// that context says they stand for.
///
/// # Errors
///
/// Returns [`ReceivedEntityRefusal`] when the entity's own context does not
/// process, the entity is no entity under it, or it does not expand.
pub(crate) async fn read_notified(
    engine: &JsonLdEngine,
    inbound: &EntityExpansion<'_>,
    contextual: &ContextualEntity,
) -> Result<ReceivedDocument, ReceivedEntityRefusal> {
    match &contextual.context {
        None => {
            let entity = contextual.entity(inbound.context().reading()).map_err(ReceivedEntityRefusal::Undecodable)?;
            Ok(inbound.notified(entity).await?)
        }
        Some(own) => {
            // clone: the context specification owns the literal it names
            let expansion = EntityExpansion::prepare(engine, &ContextSpec::from_body(own.clone()))
                .await
                .map_err(ReceivedEntityRefusal::OwnContext)?;
            let entity = contextual.entity(expansion.context().reading()).map_err(ReceivedEntityRefusal::Undecodable)?;
            Ok(expansion.notified(entity).await?)
        }
    }
}

/// One deletion a sender notified (clause 5.8.6).
#[derive(Debug)]
pub(crate) enum NotifiedDeletion {
    /// The whole attribute, notified as the NGSI-LD Null in place of the
    /// member.
    Member(AttributeName),
    /// One instance, notified as an object whose value member is its family's
    /// deletion encoding: the form a deletion takes when a `datasetId`, the
    /// system attributes or a previous value are reported with it.
    Instance(AttributeName, Box<InstanceDocument>),
}

/// The notified part of a split entity, set apart into the version its other
/// holders' versions merge onto, the deletions it notified and the previous
/// values it reported.
#[derive(Debug)]
pub(crate) struct NotifiedPart {
    /// The part as a version of the entity.
    pub(crate) version: ReceivedState,
    /// What the sender notified deleted, in notified order.
    pub(crate) deletions: Vec<NotifiedDeletion>,
    /// The instances the change touched as the sender reported they stood
    /// before it.
    pub(crate) previous: Option<EntityDocument>,
}

impl NotifiedPart {
    /// Set `part` apart.
    ///
    /// # Errors
    ///
    /// Returns [`ReceivedEntityRefusal::Incomplete`] for a part that, its
    /// deletions set aside, is no entity: one that deletes its `scope`.
    pub(crate) fn of(part: ReceivedDocument) -> Result<NotifiedPart, ReceivedEntityRefusal> {
        let ReceivedDocument {
            document,
            system_times,
            previous,
        } = part;
        let parts = document.into_parts();
        let mut deletions = Vec::new();
        let mut kept = IndexMap::with_capacity(parts.attributes.len());
        for (name, member) in parts.attributes {
            match member {
                AttributeMember::Remove => deletions.push(NotifiedDeletion::Member(name)),
                AttributeMember::Instances(instances) => {
                    let (removed, held): (Vec<InstanceDocument>, Vec<InstanceDocument>) =
                        instances.into_iter().partition(|instance| matches!(instance.value(), ValueSlot::Remove(_)));
                    // clone: each instance deletion is put back under its attribute after the merge
                    deletions.extend(removed.into_iter().map(|instance| NotifiedDeletion::Instance(name.clone(), Box::new(instance))));
                    if !held.is_empty() {
                        kept.insert(name, AttributeMember::Instances(held));
                    }
                }
            }
        }
        let state = EntityState::try_from(EntityDocument::new(parts.id, parts.types, parts.scope, parts.expires_at, kept))
            .map_err(ReceivedEntityRefusal::Incomplete)?;
        Ok(NotifiedPart {
            version: ReceivedState {
                state,
                system_times,
                linked: Vec::new(),
            },
            deletions,
            previous,
        })
    }

    /// The completed entity `merged` is, as the written document the local
    /// filters read, the notified deletions put back in place of whatever
    /// another holder holds under the same attribute and `datasetId`, and the
    /// reported previous values beside it.
    pub(crate) fn completed(merged: ReceivedState, deletions: Vec<NotifiedDeletion>, previous: Option<EntityDocument>) -> ReceivedDocument {
        let ReceivedState { mut state, system_times, .. } = merged;
        let attributes = state.attributes_mut();
        for deletion in &deletions {
            match deletion {
                NotifiedDeletion::Member(name) => {
                    attributes.remove(name);
                }
                NotifiedDeletion::Instance(name, instance) => {
                    let emptied = attributes
                        .get_mut(name)
                        .is_some_and(|held| matches!(held.remove(instance.dataset_id()), InstanceRemoval::Emptied(_)));
                    if emptied {
                        attributes.remove(name);
                    }
                }
            }
        }
        let mut document = EntityDocument::from(&state);
        for deletion in deletions {
            match deletion {
                NotifiedDeletion::Member(name) => {
                    document.attributes_mut().insert(name, AttributeMember::Remove);
                }
                NotifiedDeletion::Instance(name, instance) => match document.attributes_mut().entry(name) {
                    Entry::Occupied(mut held) => match held.get_mut() {
                        AttributeMember::Instances(instances) => instances.push(*instance),
                        AttributeMember::Remove => {}
                    },
                    Entry::Vacant(absent) => {
                        absent.insert(AttributeMember::Instances(vec![*instance]));
                    }
                },
            }
        }
        ReceivedDocument {
            document,
            system_times,
            previous,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        received_entity::read_notified,
        relay_fixture::{offline_engine, received, relayed, relayed_under, relayed_with_changes},
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_compaction::entity_compactor::EntityCompactor;
    use andromeda_ngsi_ld_expansion::entity_expansion::EntityExpansion;
    use andromeda_ngsi_ld_model::received::{ReceivedElement, ReceivedEntities};
    use andromeda_ngsi_ld_query::projection::compiled::Projection;
    use andromeda_ngsi_ld_render::{
        notification_entity::NotificationEntity,
        notification_language::NotificationLanguage,
        structural_projection::StructuralProjection,
    };
    use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
    use serde_json::json;

    /// Table 5.2.12-1 `lang` and clause 4.15: a relayed `LanguageProperty` is
    /// reduced to the subscription's language, in every representation that
    /// writes it, and is untouched without one.
    #[tokio::test]
    async fn a_relayed_language_property_is_reduced_under_the_subscription_lang() {
        let notified = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "name": {"type": "LanguageProperty", "languageMap": {"en": "Tower", "de": "Turm"}}
        });
        let english = NotificationLanguage::of(&"en".parse().expect("a language tag"));
        let reduced = relayed_under(&notified, ResponseFormat::Normalized, Some(&english)).await;
        pretty_assertions::assert_eq!(reduced["name"], json!({"type": "Property", "value": "Tower", "lang": "en"}), "{reduced}");
        let simplified = relayed_under(&notified, ResponseFormat::Simplified, Some(&english)).await;
        pretty_assertions::assert_eq!(simplified["name"], json!("Tower"), "{simplified}");
        let untouched = relayed(&notified, ResponseFormat::Normalized).await;
        pretty_assertions::assert_eq!(untouched["name"]["type"], json!("LanguageProperty"), "{untouched}");
    }

    /// Table 5.2.14-1 `showChanges`: the previous members a source reports are
    /// read apart from the entity, which no state holding them could be, and
    /// relayed beside the instance of the same `datasetId`.
    #[tokio::test]
    async fn reported_previous_values_are_read_apart_and_relayed_beside_their_instances() {
        let engine = offline_engine();
        let notified = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 5, "previousValue": 4},
                {"type": "Property", "value": 50, "previousValue": 40, "datasetId": "urn:ngsi-ld:Dataset:kmh"}
            ],
            "owner": {"type": "Relationship", "object": "urn:ngsi-ld:Person:b", "previousObject": "urn:ngsi-ld:Person:a"},
            "colour": {"type": "Property", "value": "red"}
        });
        let received = received(&engine, &notified).await;
        let without_changes = relayed(&notified, ResponseFormat::Normalized).await;
        assert!(without_changes["owner"].get("previousObject").is_none(), "{without_changes}");

        let written = relayed_with_changes(&engine, &received, ResponseFormat::Normalized).await;
        pretty_assertions::assert_eq!(
            written["speed"],
            json!([
                {"type": "Property", "value": 5, "previousValue": 4},
                {"type": "Property", "value": 50, "previousValue": 40, "datasetId": "urn:ngsi-ld:Dataset:kmh"}
            ]),
            "{written}"
        );
        pretty_assertions::assert_eq!(
            written["owner"],
            json!({"type": "Relationship", "object": "urn:ngsi-ld:Person:b", "previousObject": "urn:ngsi-ld:Person:a"})
        );
        pretty_assertions::assert_eq!(written["colour"], json!({"type": "Property", "value": "red"}));
    }

    /// JSON-LD 1.1 with clause 5.8.6: an entity that carries its own `@context`
    /// is read under it, and the local subscription writes the entity under its
    /// own context, so a name the sender aliased differently is the local name.
    #[tokio::test]
    async fn received_entities_are_read_under_their_own_context() {
        let engine = offline_engine();
        let inbound = EntityExpansion::prepare(&engine, &ContextSpec::core_only())
            .await
            .expect("the core context processes");
        let elements = ReceivedEntities::from_slice(
            json!({
                "@context": {"spd": "https://example.org/vocab/speed"},
                "id": "urn:ngsi-ld:Vehicle:1",
                "type": "Vehicle",
                "spd": {"type": "Property", "value": 5}
            })
            .to_string()
            .as_bytes(),
        )
        .expect("an entity");
        let [ReceivedElement::Read(contextual)] = elements.0.as_slice() else {
            panic!("the notified entity reads");
        };
        let received = read_notified(&engine, &inbound, contextual).await.expect("the notified entity expands");

        let local = ContextSpec::from_body(JsonLiteral::admit(r#"{"speed": "https://example.org/vocab/speed"}"#).expect("a context"));
        let local_compactor = engine.compactor(&local);
        let respelled = EntityCompactor::new(&local_compactor)
            .document_entity(&received.document)
            .await
            .expect("the entity compacts");
        let projection = Projection::default();
        let written = serde_json::to_value(NotificationEntity::relayed(
            &respelled,
            ResponseFormat::Normalized,
            StructuralProjection::from(&projection),
            None,
        ))
        .expect("the entity writes");

        pretty_assertions::assert_eq!(written["speed"], json!({"type": "Property", "value": 5}), "{written}");
        assert!(written.get("spd").is_none(), "{written}");
    }

    /// What a relayed notification makes of every attribute member shape
    /// (clause 5.8.6): a member read typed is written in every representation
    /// a local subscription may ask for, and a refused one refuses its entity.
    mod shapes {
        use crate::relay_fixture::relay_outcome;
        use andromeda_json_literal::JsonLiteral;
        use andromeda_jsonld::context_spec::ContextSpec;
        use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
        use serde_json::Value;

        /// What a route makes of a member written in one shape.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum Verdict {
            /// The member is read into the typed model.
            Typed,
            /// The member is refused, and the entity carrying it with it.
            Refused,
        }

        /// The user `@context` aliasing `@type`, `@id` and `ngsi-ld:hasValue`.
        const ALIASING_CONTEXT: &str = r#"{"kind": "@type", "ident": "@id", "v": "https://uri.etsi.org/ngsi-ld/hasValue"}"#;

        /// Assert what relaying an entity carrying `member`, written under the
        /// user `@context` `terms` or the Core `@context` alone, makes of it.
        async fn assert_verdict(member: &str, terms: Option<&str>, verdict: Verdict) {
            let inbound = terms.map_or_else(ContextSpec::core_only, |terms| {
                ContextSpec::from_body(JsonLiteral::admit(terms).expect("the aliasing context is JSON"))
            });
            let entity: Value =
                serde_json::from_str(&format!(r#"{{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","attr":{member}}}"#)).expect("the member is JSON");
            for format in [ResponseFormat::Normalized, ResponseFormat::Concise, ResponseFormat::Simplified] {
                let outcome = relay_outcome(&entity, &inbound, format, None).await;
                match verdict {
                    Verdict::Typed => match outcome {
                        Ok(relayed) => assert!(relayed.get("attr").is_some(), "{format:?}: {relayed}"),
                        Err(refusal) => panic!("{format:?}: {refusal}"),
                    },
                    Verdict::Refused => assert!(outcome.is_err(), "{format:?}: {outcome:?}"),
                }
            }
        }

        /// One test per shape, named after it: `member` written under the
        /// optional user `@context`, with what this route makes of it.
        macro_rules! shape_tests {
            ($($(#[$doc:meta])* $test:ident: $member:literal $(under $context:ident)? => $verdict:ident),+ $(,)?) => {
                $(
                    $(#[$doc])*
                    #[tokio::test]
                    async fn $test() {
                        assert_verdict($member, [$($context)?].into_iter().next(), Verdict::$verdict).await;
                    }
                )+
            };
        }

        shape_tests! {
            /// Clause 4.5.2.3: a bare scalar is a concise Property.
            a_bare_scalar: "5" => Typed,
            /// Clause 4.5.2.3: an array of values is a concise Property.
            an_array_of_values: "[1, 2, 3]" => Typed,
            /// Clause 4.5.2.3: the empty array is a concise Property too.
            an_empty_array: "[]" => Typed,
            /// Clause 4.5.2.3: an object naming no type is a concise Property.
            a_typeless_object: r#"{"street": "Main", "no": 1}"# => Typed,
            /// Clause 4.5.2.3: a typeless instance carrying `value`.
            a_typeless_instance_with_a_value: r#"{"value": 21, "unitCode": "CEL"}"# => Typed,
            /// Clause 4.7.3: a typeless instance whose value is a geometry.
            a_typeless_instance_with_a_geometry_value: r#"{"value": {"type": "Point", "coordinates": [13.4, 52.5]}}"# => Typed,
            /// Clause 4.5.3.3: a typeless instance carrying `object`.
            a_typeless_instance_with_an_object: r#"{"object": "urn:ngsi-ld:Owner:1"}"# => Typed,
            /// Clause 4.5.18.3: a typeless instance carrying `languageMap`.
            a_typeless_instance_with_a_language_map: r#"{"languageMap": {"en": "Tower", "de": "Turm"}}"# => Typed,
            /// Clause 4.5.20.3: a typeless instance carrying `vocab`.
            a_typeless_instance_with_a_vocab: r#"{"vocab": "Tower"}"# => Typed,
            /// Clause 4.5.21.3: a typeless instance carrying `valueList`.
            a_typeless_instance_with_a_value_list: r#"{"valueList": [1, 2]}"# => Typed,
            /// Clause 4.5.22.3: a typeless instance carrying `objectList`.
            a_typeless_instance_with_an_object_list: r#"{"objectList": ["urn:ngsi-ld:Owner:1"]}"# => Typed,
            /// Clause 4.5.24.3: a typeless instance carrying `json`.
            a_typeless_instance_with_a_json_value: r#"{"json": {"a": [1, {"b": null}]}}"# => Typed,
            /// Clause 4.7.3: a bare geometry is a concise `GeoProperty`.
            a_bare_geometry: r#"{"type": "Point", "coordinates": [13.4, 52.5]}"# => Typed,
            /// Clause 4.5.5: a multi-attribute one of whose instances is concise.
            an_array_with_a_concise_instance: r#"[{"value": 1, "datasetId": "urn:ngsi-ld:Dataset:a"}, {"type": "Property", "value": 2}]"# => Typed,
            /// Clause 4.7.2: a geometry may be written as the JSON string encoding it.
            a_geometry_written_as_a_string: r#"{"type": "GeoProperty", "value": "{\"type\":\"Point\",\"coordinates\":[1,2]}"}"# => Typed,
            /// Clause 4.7.2: a string that encodes no geometry is no `GeoProperty` value.
            a_string_encoding_no_geometry: r#"{"type": "GeoProperty", "value": "not a geometry"}"# => Refused,
            /// Clause 5.5.4 refuses the NGSI-LD Null outside a merge, which removes the
            /// member (clause 5.5.12), and a notification reports a deleted attribute so
            /// (clause 5.8.6).
            a_bare_ngsi_ld_null: r#""urn:ngsi-ld:null""# => Typed,
            /// Clause 4.5.0 uses the NGSI-LD Null as the value of a deleted `GeoProperty`
            /// in a notification and a temporal representation; clause 5.5.4 refuses it
            /// elsewhere outside a merge.
            a_geoproperty_whose_value_is_the_null: r#"{"type": "GeoProperty", "value": "urn:ngsi-ld:null"}"# => Typed,
            /// Clause 5.5.12: a merge removes the `unitCode` written as the NGSI-LD Null.
            a_null_unit_code: r#"{"type": "Property", "value": 1, "unitCode": "urn:ngsi-ld:null"}"# => Refused,
            /// Clause 5.5.12: a merge removes the `observedAt` written as the NGSI-LD Null.
            a_null_observed_at: r#"{"type": "Property", "value": 1, "observedAt": "urn:ngsi-ld:null"}"# => Refused,
            /// Clause 5.5.12, last bullet: a `datasetId` is never removed by a merge.
            a_null_dataset_id: r#"{"type": "Property", "value": 1, "datasetId": "urn:ngsi-ld:null"}"# => Refused,
            /// Clauses 4.5.0 and 5.2.1: no member admits JSON `null`.
            a_json_null: "null" => Refused,
            /// Clause 4.5.2.2: a type naming no attribute type.
            an_unknown_type: r#"{"type": "Propertyy", "value": 1}"# => Refused,
            /// Clause 4.7.3: a geometry type without its coordinates.
            a_geometry_without_coordinates: r#"{"type": "Point"}"# => Refused,
            /// Clause 4.5.2.2 requires the value member; clause 5.5.12 lets a merge keep
            /// the value held.
            an_instance_without_its_value: r#"{"type": "Property"}"# => Refused,
            /// Clause 4.5.3.2: an `object` is a URI.
            an_undecodable_family_value: r#"{"type": "Relationship", "object": "not a uri"}"# => Refused,
            /// Clause 4.8: an `observedAt` is a `DateTime`.
            an_undecodable_envelope_member: r#"{"type": "Property", "value": 1, "observedAt": "yesterday"}"# => Refused,
            /// Clause 5.2.2: a producer's system member is ignored; a peer's own system
            /// members are `DateTime`s (clause 4.8).
            a_malformed_system_member: r#"{"type": "Property", "value": 1, "createdAt": "not a time"}"# => Refused,
            /// Clause 4.6.2: a sub-attribute is named by an attribute name.
            a_bad_sub_attribute_name: r#"{"type": "Property", "value": 1, "bad name": {"type": "Property", "value": 2}}"# => Refused,
            /// Clause 4.5.5: a multi-attribute is an array of instances.
            a_mixed_array: r#"[{"type": "Property", "value": 1}, 1]"# => Refused,
            /// Clause 4.5.2.3: a concise instance infers one family.
            several_family_value_members: r#"{"object": "urn:ngsi-ld:Owner:1", "vocab": "Tower"}"# => Refused,
            /// Clause 5.5.7: no JSON-LD keyword inside a geometry.
            a_geometry_with_a_json_ld_keyword: r#"{"type": "GeoProperty", "value": {"@id": "urn:ngsi-ld:Geometry:1", "type": "Point", "coordinates": [13.4, 52.5]}}"# => Refused,
            /// Clause 4.4: `@type` stands for `type`.
            an_instance_spelled_with_keywords: r#"{"@type": "Property", "value": 1}"# => Typed,
            /// Clause 5.5.7: a user `@context` may alias `@type` and `ngsi-ld:hasValue`.
            an_instance_spelled_with_user_aliases: r#"{"kind": "Property", "v": 1}"# under ALIASING_CONTEXT => Typed,
        }
    }
}
