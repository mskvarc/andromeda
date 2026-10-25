//! The entities one notification carries, written once the way clause 5.8.6
//! sends them: each notified entity in the subscription's format with the Linked
//! Entities of its `join` nested into it for `inline` or written beside it for
//! `flat` (clause 4.5.23), `pick` and `omit` applied, and every entity amended
//! to the subscription's `ngsildConformance` as it is written. Which Linked
//! Entities are
//! written, where, and narrowed by which list is the engine's plan under the
//! subscription's list, which "every Entity within the payload body" is
//! reduced by (clause 5.8.6).
//!
//! A change-driven notification carries the one entity the change touched, a
//! periodic one every entity the subscription matches; both are written here,
//! so the two cannot send the same subscription differently.

use crate::{
    error::NotificationError,
    processor::payload::notified_entity::{NotifiedEntity, WrittenShape, linked_entity},
};
use andromeda_linked_retrieval::{
    graph::LinkedGraph,
    narrowing::{list::ListInForce, member::MemberRule},
};
use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
use andromeda_ngsi_ld_model::{entity::Entity, notification::NotificationData};
use andromeda_ngsi_ld_query::projection::attribute_iri_index::AttributeIriIndex;
use andromeda_ngsi_ld_render::{
    joined_notification::{join_plan, joined_entities},
    notification_data::{DataShape, notification_data},
    notification_language::NotificationLanguage,
    structural_projection::StructuralProjection,
    version::{adapted_payload::AdaptedPayload, fallback::VersionFallback},
};
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, response_format::ResponseFormat};

/// How a subscription has the entities of its notifications written.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CarriedShape<'a> {
    /// The `notification.format`.
    pub(crate) format: ResponseFormat,
    /// The `notification.join`.
    pub(crate) join: Option<JoinMode>,
    /// The `id` / `type` / `scope` half of the `pick` / `omit` projection.
    pub(crate) projection: StructuralProjection<'a>,
    /// The list in force the subscription narrows every entity by.
    pub(crate) list: ListInForce<'a>,
    /// The `ngsildConformance` the entities are amended to.
    pub(crate) conformance: Option<SpecVersion>,
    /// The subscription's `lang` every entity is reduced under (Table
    /// 5.2.12-1, clause 4.15).
    pub(crate) language: Option<&'a NotificationLanguage>,
}

/// The Linked Entities a subscription's `join` reached, with the expansion of
/// every attribute key the notification carries.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NotifiedLinks<'a> {
    /// What the traversal reached.
    pub(crate) graph: &'a LinkedGraph<Entity>,
    /// The expansion of every attribute key of the roots and the graph.
    pub(crate) index: &'a AttributeIriIndex,
}

/// The `data` of a notification carrying every notified entity, with the
/// `linked` entities of a `join` nested into it or written after them all,
/// written once as `data` says.
///
/// Clause 4.21, the `id` / `type` / `scope` half of the subscription's
/// projection, narrows each notified entity's `type` and `scope`; clause 5.8.6
/// holds `id` back from that rule, since a notification carries Entities and
/// an Entity is only actionable with its `id` (clause 4.5.1). Clause 5.8.6
/// amends "every Entity within the payload body" to the `ngsildConformance`
/// version: the fallback tables of clause 4.3.6.8 are applied by the walk that
/// writes each entity in the format the subscriber receives, to the Linked
/// Entities nested into it and to the flat ones beside it alike.
///
/// `linked` is `None` when no traversal ran or it failed, which writes the
/// notified entities alone whatever the `join`. Which Linked Entities are
/// written, and where, is the engine's plan over `linked`.
///
/// # Errors
///
/// Returns [`NotificationError::Rendering`] when an entity cannot be written.
pub(crate) fn carried_entities(
    notified: &[NotifiedEntity],
    linked: Option<NotifiedLinks<'_>>,
    shape: CarriedShape<'_>,
    data: DataShape<'_>,
) -> Result<NotificationData, NotificationError> {
    let CarriedShape {
        format,
        join,
        projection,
        list,
        conformance,
        language,
    } = shape;
    let fallback = conformance.and_then(|target| VersionFallback::new(target, AdaptedPayload::Entity));
    let plan = linked.and_then(|linked| join_plan(join, Some(linked.graph), MemberRule::new(list, linked.index)));
    let written = WrittenShape { format, fallback, language };
    let entities = joined_entities(
        plan.as_ref(),
        notified,
        |entity, cursor| entity.notified(written, cursor, projection),
        |linked| linked_entity(linked, written),
    );
    Ok(notification_data(&entities, data)?)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::processor::payload::{
        carried_entities::{CarriedShape, NotifiedLinks, carried_entities},
        notified_entity::NotifiedEntity,
    };
    use andromeda_http_headers::media_type::ResponseMediaType;
    use andromeda_linked_retrieval::{
        graph::{LinkedGraph, RootEdges},
        narrowing::list::ListInForce,
        typed::TypedEntities,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, spec_version::SpecVersion};
    use andromeda_ngsi_ld_model::entity::Entity;
    use andromeda_ngsi_ld_query::projection::{attribute_iri_index::AttributeIriIndex, compiled::Projection, term::ProjectionTerm};
    use andromeda_ngsi_ld_render::{
        geometry_selection::GeometrySelection,
        notification_data::DataShape,
        notification_entity::StampedInstants,
        notification_language::NotificationLanguage,
        notified_changes::HeldAttributes,
        structural_projection::StructuralProjection,
    };
    use andromeda_ngsi_ld_representation::{join_mode::JoinMode, response_format::ResponseFormat};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::{num::NonZeroU32, sync::Arc};

    fn entity(document: &Value) -> Entity {
        serde_json::from_str(&document.to_string()).expect("the entity decodes")
    }

    fn notified(document: &Value) -> NotifiedEntity {
        NotifiedEntity {
            entity: Arc::new(entity(document)),
            stamped: StampedInstants::default(),
            held: HeldAttributes::Current,
            deleted: None,
            previous: None,
        }
    }

    /// Two Buildings in one City, as a periodic notification carries them.
    fn buildings() -> Vec<NotifiedEntity> {
        ["urn:ngsi-ld:Building:1", "urn:ngsi-ld:Building:2"]
            .iter()
            .map(|id| {
                notified(&json!({
                    "id": id,
                    "type": "Building",
                    "locatedAt": {"type": "Relationship", "object": "urn:ngsi-ld:City:Paris"}
                }))
            })
            .collect()
    }

    /// The graph a one-level join from `notified` reaches: the City.
    fn city(notified: &[NotifiedEntity]) -> LinkedGraph<Entity> {
        city_as(
            notified,
            &json!({
                "id": "urn:ngsi-ld:City:Paris",
                "type": "City",
                "name": {"type": "Property", "value": "Paris"},
                "mayor": {"type": "Property", "value": "someone"}
            }),
        )
    }

    /// The graph a one-level join from `notified` reaches: the City, as
    /// `document` writes it.
    fn city_as(notified: &[NotifiedEntity], document: &Value) -> LinkedGraph<Entity> {
        let RootEdges { ids, edges } = RootEdges::of::<TypedEntities>(notified.iter().map(|entity| &*entity.entity));
        LinkedGraph {
            roots: ids,
            nodes: [("urn:ngsi-ld:City:Paris".parse().expect("an identifier"), entity(document))]
                .into_iter()
                .collect(),
            edges,
            failures: Vec::new(),
            depth: NonZeroU32::MIN,
        }
    }

    fn iri(key: &str) -> IriBuf {
        IriBuf::new(format!("https://example.com/{key}")).expect("valid IRI")
    }

    /// The expansion of every key the Buildings and the City carry.
    fn index() -> AttributeIriIndex {
        let mut index = AttributeIriIndex::new();
        for key in ["locatedAt", "name", "mayor"] {
            index.insert(key.parse::<AttributeName>().expect("valid attribute name"), iri(key));
        }
        index
    }

    fn leaf(key: &str) -> ProjectionTerm {
        ProjectionTerm::Attribute {
            iri: iri(key),
            linked: Vec::new(),
        }
    }

    fn shape(join: Option<JoinMode>, projection: &Projection) -> CarriedShape<'_> {
        CarriedShape {
            format: ResponseFormat::Normalized,
            join,
            projection: StructuralProjection::from(projection),
            list: ListInForce::of(Some(projection), &[]),
            conformance: None,
            language: None,
        }
    }

    const fn links<'a>(graph: &'a LinkedGraph<Entity>, index: &'a AttributeIriIndex) -> NotifiedLinks<'a> {
        NotifiedLinks { graph, index }
    }

    /// The entities `carried_entities` writes into an `application/json`
    /// notification's `data`.
    fn carried(notified: &[NotifiedEntity], linked: Option<NotifiedLinks<'_>>, shape: CarriedShape<'_>) -> Vec<Value> {
        let data = DataShape {
            media_type: ResponseMediaType::Json,
            selection: GeometrySelection::new(None, &[]),
            context: None,
        };
        let written = carried_entities(notified, linked, shape, data).expect("the walk writes them");
        serde_json::from_str(written.text()).expect("the written data is an Entity array")
    }

    fn ids(payload: &[Value]) -> Vec<&str> {
        payload.iter().filter_map(|entity| entity["id"].as_str()).collect()
    }

    /// Clause 4.5.23.3: a flat join writes the Linked Entity once, after every
    /// notified entity.
    #[test]
    fn a_flat_join_writes_the_linked_entities_after_every_notified_entity() {
        let projection = Projection::default();
        let buildings = buildings();
        let linked = city(&buildings);
        let index = index();
        let payload = carried(&buildings, Some(links(&linked, &index)), shape(Some(JoinMode::Flat), &projection));
        assert_eq!(
            ids(&payload),
            vec!["urn:ngsi-ld:Building:1", "urn:ngsi-ld:Building:2", "urn:ngsi-ld:City:Paris"]
        );
    }

    /// Clause 4.5.23.2: an inline join nests the Linked Entity under the
    /// Relationship of every notified entity naming it.
    #[test]
    fn an_inline_join_nests_the_linked_entity_into_every_notified_entity() {
        let projection = Projection::default();
        let buildings = buildings();
        let linked = city(&buildings);
        let index = index();
        let payload = carried(&buildings, Some(links(&linked, &index)), shape(Some(JoinMode::Inline), &projection));
        assert_eq!(ids(&payload), vec!["urn:ngsi-ld:Building:1", "urn:ngsi-ld:Building:2"]);
        for building in &payload {
            assert_eq!(building["locatedAt"]["entity"]["id"], json!("urn:ngsi-ld:City:Paris"));
        }
    }

    /// No traversal, or `join=@none`, writes the notified entities alone.
    #[test]
    fn without_linked_entities_only_the_notified_entities_are_written() {
        let projection = Projection::default();
        let buildings = buildings();
        let linked = city(&buildings);
        let index = index();
        for (join, linked) in [(Some(JoinMode::Flat), None), (Some(JoinMode::None), Some(links(&linked, &index))), (None, None)] {
            let payload = carried(&buildings, linked, shape(join, &projection));
            assert_eq!(ids(&payload), vec!["urn:ngsi-ld:Building:1", "urn:ngsi-ld:Building:2"], "{join:?}");
            assert!(payload[0]["locatedAt"].get("entity").is_none());
        }
    }

    /// Clause 4.21 with clause 5.8.6: `omit` of `type` and `id` removes both
    /// from every entity of the payload, except the notified entities' `id`,
    /// which clause 5.8.6 holds back from the rule.
    #[test]
    fn the_structural_projection_narrows_every_entity_of_the_payload() {
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![ProjectionTerm::Type, ProjectionTerm::Id],
        };
        let buildings = buildings();
        let linked = city(&buildings);
        let index = index();
        let payload = carried(&buildings, Some(links(&linked, &index)), shape(Some(JoinMode::Flat), &projection));
        assert!(payload[0].get("type").is_none() && payload[1].get("type").is_none());
        assert_eq!(payload[0]["id"], json!("urn:ngsi-ld:Building:1"));
        assert_eq!(
            payload[2],
            json!({"name": {"type": "Property", "value": "Paris"}, "mayor": {"type": "Property", "value": "someone"}})
        );
    }

    /// Clause 5.8.6, "every Entity within the payload body": a plain `pick`
    /// narrows the flat Linked Entity as it narrows the notified entities.
    #[test]
    fn a_plain_pick_narrows_a_flat_linked_entity() {
        let projection = Projection::builder().pick(vec![ProjectionTerm::Id, leaf("name"), leaf("locatedAt")]).build();
        let buildings = buildings();
        let linked = city(&buildings);
        let index = index();
        let payload = carried(&buildings, Some(links(&linked, &index)), shape(Some(JoinMode::Flat), &projection));
        assert_eq!(
            payload[2],
            json!({"id": "urn:ngsi-ld:City:Paris", "name": {"type": "Property", "value": "Paris"}})
        );
    }

    /// Clause 5.8.6 amends "every Entity within the payload body" to the
    /// `ngsildConformance` version: the notified entities, the Linked Entity
    /// an inline join nests into them, and the one a flat join writes beside
    /// them.
    #[test]
    fn the_conformance_amends_every_entity_of_the_payload() {
        let projection = Projection::default();
        let buildings = vec![notified(&json!({
            "id": "urn:ngsi-ld:Building:1",
            "type": ["Building", "Landmark"],
            "locatedAt": {"type": "Relationship", "object": "urn:ngsi-ld:City:Paris", "observedAt": "2025-01-01T00:00:00Z"}
        }))];
        let linked = city_as(
            &buildings,
            &json!({
                "id": "urn:ngsi-ld:City:Paris",
                "type": "City",
                "scope": "/France",
                "name": {"type": "LanguageProperty", "languageMap": {"fr": "Paris"}}
            }),
        );
        let index = index();
        let amended = |join| CarriedShape {
            conformance: Some(SpecVersion::V1_0),
            ..shape(Some(join), &projection)
        };
        let flat = carried(&buildings, Some(links(&linked, &index)), amended(JoinMode::Flat));
        assert_eq!(flat[0]["type"], json!("Building"));
        assert_eq!(flat[0]["locatedAt"], json!({"type": "Relationship", "object": "urn:ngsi-ld:City:Paris"}));
        assert_eq!(
            flat[1],
            json!({"id": "urn:ngsi-ld:City:Paris", "type": "City", "name": {"type": "Property", "value": {"fr": "Paris"}}})
        );
        let inline = carried(&buildings, Some(links(&linked, &index)), amended(JoinMode::Inline));
        assert_eq!(
            inline[0]["locatedAt"]["entity"],
            json!({"id": "urn:ngsi-ld:City:Paris", "type": "City", "name": {"type": "Property", "value": {"fr": "Paris"}}})
        );
    }

    /// Table 5.2.12-1 `lang` with clause 4.15: the subscription's language
    /// reduces every `LanguageProperty` of a notified entity, and a
    /// subscription naming none leaves the language map whole.
    #[test]
    fn the_subscription_lang_reduces_every_language_property_of_the_payload() {
        let projection = Projection::default();
        let places = vec![notified(&json!({
            "id": "urn:ngsi-ld:Place:1",
            "type": "Place",
            "name": {"type": "LanguageProperty", "languageMap": {"en": "Tower", "de": "Turm"}}
        }))];
        let german = NotificationLanguage::of(&"de".parse().expect("a language tag"));
        let reduced = carried(
            &places,
            None,
            CarriedShape {
                language: Some(&german),
                ..shape(None, &projection)
            },
        );
        assert_eq!(reduced[0]["name"], json!({"type": "Property", "value": "Turm", "lang": "de"}));
        let whole = carried(&places, None, shape(None, &projection));
        assert_eq!(whole[0]["name"]["type"], json!("LanguageProperty"));
    }

    /// A Linked Entity the list leaves no member is not written, and the
    /// Relationship keeps the `object` naming it.
    #[test]
    fn an_emptied_linked_entity_is_not_written() {
        let projection = Projection::builder().pick(vec![leaf("locatedAt")]).build();
        let buildings = buildings();
        let linked = city(&buildings);
        let index = index();
        let flat = carried(&buildings, Some(links(&linked, &index)), shape(Some(JoinMode::Flat), &projection));
        assert_eq!(flat.len(), 2);
        let inline = carried(&buildings, Some(links(&linked, &index)), shape(Some(JoinMode::Inline), &projection));
        assert_eq!(inline[0]["locatedAt"], json!({"type": "Relationship", "object": "urn:ngsi-ld:City:Paris"}));
    }
}
