//! Table 4.3.6.8-1 reformats a `ListRelationship` as a Relationship for a
//! target before 1.8, and Table 4.3.6.8-3 types the Relationship's `object`
//! "String or String[]", so every target of the list is kept, in list order,
//! in a response and a notification, with and without an inline join.

use crate::{
    context::ResponseContext,
    entity_response::{EntityBody, EntityPayload},
    notification_entity::{NotificationEntity, StampedInstants},
    notified_changes::{HeldAttributes, NotifiedChanges},
    response::ResponseRenderer,
    version::{adapted_payload::AdaptedPayload, fallback::VersionFallback},
};
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_linked_retrieval::{
    graph::{LinkedGraph, RootEdges},
    join::RetrievingJoin,
    narrowing::{list::ListInForce, member::MemberRule},
    plan::JoinPlan,
    representation::LinkedRepresentation,
    typed::TypedEntities,
};
use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::projection::attribute_iri_index::AttributeIriIndex;
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::num::NonZeroU32;

/// The version every case is written for, the last one before 1.8.
const TARGET: SpecVersion = SpecVersion::new(1, 7);

/// Three targets, out of identifier order so the list order shows.
const THREE: [&str; 3] = ["urn:ngsi-ld:Stop:3", "urn:ngsi-ld:Stop:1", "urn:ngsi-ld:Stop:2"];

/// An entity from its normalized document.
fn entity(document: Value) -> Entity {
    serde_json::from_value(document).expect("the entity decodes")
}

/// A Route whose `stops` lists `targets`.
fn route(targets: &[&str]) -> Entity {
    entity(json!({
        "id": "urn:ngsi-ld:Route:1",
        "type": "Route",
        "stops": {"type": "ListRelationship", "objectList": targets}
    }))
}

/// A Stop, the entity a target of `stops` names.
fn stop(id: &str) -> Entity {
    entity(json!({"id": id, "type": "Stop", "name": {"type": "Property", "value": id}}))
}

/// The `stops` member of `body`, written in `format` for [`TARGET`].
fn stops(body: EntityBody<'_>, format: ResponseFormat) -> Value {
    let mut written = Vec::new();
    ResponseRenderer::new(ResponseMediaType::Json, ResponseContext::Absent)
        .with_format(format)
        .with_requested_version(Some(TARGET))
        .with_entities(EntityPayload::Single(body), None)
        .render_into(&mut written)
        .expect("the entity is written");
    let mut document: Value = serde_json::from_slice(&written).expect("the body is JSON");
    document["stops"].take()
}

/// The graph an inline join one level deep from `root` holds, every Stop of
/// [`THREE`] resolved.
fn graph(root: &Entity) -> LinkedGraph<Entity> {
    let nodes: Vec<Entity> = THREE.into_iter().map(stop).collect();
    let RootEdges { ids, mut edges } = RootEdges::of::<TypedEntities>([root]);
    for node in &nodes {
        TypedEntities::edges(node, &mut edges);
    }
    LinkedGraph {
        roots: ids,
        nodes: nodes.into_iter().map(|node| (node.id.clone(), node)).collect(),
        edges,
        failures: Vec::new(),
        depth: NonZeroU32::MIN,
    }
}

/// The identifiers of the entities `entity` nests, in order.
fn nested_ids(entity: &Value) -> Vec<&Value> {
    entity.as_array().expect("an array of entities").iter().map(|nested| &nested["id"]).collect()
}

#[test]
fn three_targets_are_the_object_in_list_order_in_normalized_and_concise() {
    let route = route(&THREE);
    assert_eq!(
        stops(EntityBody::new(&route, None), ResponseFormat::Normalized),
        json!({"type": "Relationship", "object": THREE})
    );
    assert_eq!(stops(EntityBody::new(&route, None), ResponseFormat::Concise), json!({"object": THREE}));
}

#[test]
fn one_target_is_the_single_object_in_normalized_and_concise() {
    let route = route(&["urn:ngsi-ld:Stop:1"]);
    assert_eq!(
        stops(EntityBody::new(&route, None), ResponseFormat::Normalized),
        json!({"type": "Relationship", "object": "urn:ngsi-ld:Stop:1"})
    );
    assert_eq!(
        stops(EntityBody::new(&route, None), ResponseFormat::Concise),
        json!({"object": "urn:ngsi-ld:Stop:1"})
    );
}

#[test]
fn no_target_is_the_null_object_in_normalized_and_concise() {
    let route = route(&[]);
    assert_eq!(
        stops(EntityBody::new(&route, None), ResponseFormat::Normalized),
        json!({"type": "Relationship", "object": null})
    );
    assert_eq!(stops(EntityBody::new(&route, None), ResponseFormat::Concise), json!({"object": null}));
}

/// Clause 4.5.3.3 writes a Relationship's simplified value as its targets, so
/// the reformat leaves the list the simplified representation writes.
#[test]
fn the_simplified_representation_keeps_every_target() {
    let route = route(&THREE);
    assert_eq!(stops(EntityBody::new(&route, None), ResponseFormat::KeyValues), json!(THREE));
}

/// Clause 4.5.21.2: the previous member has the representation of the value
/// member, so `previousObjectList` becomes `previousObject` the same way.
#[test]
fn previous_targets_become_the_previous_object_in_list_order() {
    let route = entity(json!({
        "id": "urn:ngsi-ld:Route:1",
        "type": "Route",
        "stops": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Stop:9"], "previousObjectList": THREE}
    }));
    assert_eq!(
        stops(EntityBody::new(&route, None), ResponseFormat::Normalized),
        json!({"type": "Relationship", "object": "urn:ngsi-ld:Stop:9", "previousObject": THREE})
    );
}

/// Clause 4.5.23.2 nests an array of entities for an array of targets, one
/// entry per target, in their order.
#[test]
fn an_inline_join_nests_every_target_in_list_order() {
    let route = route(&THREE);
    let graph = graph(&route);
    let index = AttributeIriIndex::new();
    let JoinPlan::Inline(plan) = JoinPlan::<TypedEntities>::new(RetrievingJoin::Inline, &graph, MemberRule::new(ListInForce::Unrestricted, &index)) else {
        panic!("an inline join plans inline");
    };
    for format in [ResponseFormat::Normalized, ResponseFormat::Concise] {
        let written = stops(EntityBody::new(&route, None).with_inline_join(plan.cursor()), format);
        assert_eq!(written["object"], json!(THREE), "{format:?}");
        assert_eq!(nested_ids(&written["entity"]), THREE, "{format:?}");
    }
}

/// Clause 4.5.23.2 nests one entity for a single target URI.
#[test]
fn an_inline_join_nests_the_one_target_as_one_entity() {
    let route = route(&["urn:ngsi-ld:Stop:1"]);
    let graph = graph(&route);
    let index = AttributeIriIndex::new();
    let JoinPlan::Inline(plan) = JoinPlan::<TypedEntities>::new(RetrievingJoin::Inline, &graph, MemberRule::new(ListInForce::Unrestricted, &index)) else {
        panic!("an inline join plans inline");
    };
    for format in [ResponseFormat::Normalized, ResponseFormat::Concise] {
        let written = stops(EntityBody::new(&route, None).with_inline_join(plan.cursor()), format);
        assert_eq!(written["entity"]["id"], "urn:ngsi-ld:Stop:1", "{format:?}");
    }
}

#[test]
fn an_inline_join_nests_nothing_for_no_target() {
    let route = route(&[]);
    let graph = graph(&route);
    let index = AttributeIriIndex::new();
    let JoinPlan::Inline(plan) = JoinPlan::<TypedEntities>::new(RetrievingJoin::Inline, &graph, MemberRule::new(ListInForce::Unrestricted, &index)) else {
        panic!("an inline join plans inline");
    };
    let written = stops(EntityBody::new(&route, None).with_inline_join(plan.cursor()), ResponseFormat::Normalized);
    assert_eq!(written, json!({"type": "Relationship", "object": null}));
}

/// The entities a `ListRelationship` already carries in `entityList` are
/// nested under `entity` as they were carried.
#[test]
fn carried_entities_are_nested_whole() {
    let route = entity(json!({
        "id": "urn:ngsi-ld:Route:1",
        "type": "Route",
        "stops": {
            "type": "ListRelationship",
            "objectList": THREE,
            "entityList": THREE.map(|id| json!({"id": id, "type": "Stop"}))
        }
    }));
    assert_eq!(nested_ids(&stops(EntityBody::new(&route, None), ResponseFormat::Normalized)["entity"]), THREE);
}

/// The `stops` member of `notified`, sent to a Subscription whose
/// `ngsildConformance` is [`TARGET`] (clause 5.8.6).
fn notified(notified: NotificationEntity<'_>) -> Value {
    let mut document =
        serde_json::to_value(notified.with_fallback(VersionFallback::new(TARGET, AdaptedPayload::Entity))).expect("the notified entity is written");
    document["stops"].take()
}

#[test]
fn a_notification_keeps_every_target_in_normalized_and_concise() {
    let stamped = StampedInstants::default();
    let three = route(&THREE);
    assert_eq!(
        notified(NotificationEntity::new(&three, &stamped, ResponseFormat::Normalized)),
        json!({"type": "Relationship", "object": THREE})
    );
    assert_eq!(
        notified(NotificationEntity::new(&three, &stamped, ResponseFormat::Concise)),
        json!({"object": THREE})
    );
    let one = route(&["urn:ngsi-ld:Stop:1"]);
    assert_eq!(
        notified(NotificationEntity::new(&one, &stamped, ResponseFormat::Normalized)),
        json!({"type": "Relationship", "object": "urn:ngsi-ld:Stop:1"})
    );
    let none = route(&[]);
    assert_eq!(
        notified(NotificationEntity::new(&none, &stamped, ResponseFormat::Normalized)),
        json!({"type": "Relationship", "object": null})
    );
}

#[test]
fn a_joined_notification_nests_every_target_in_list_order() {
    let stamped = StampedInstants::default();
    let route = route(&THREE);
    let graph = graph(&route);
    let index = AttributeIriIndex::new();
    let JoinPlan::Inline(plan) = JoinPlan::<TypedEntities>::new(RetrievingJoin::Inline, &graph, MemberRule::new(ListInForce::Unrestricted, &index)) else {
        panic!("an inline join plans inline");
    };
    let written = notified(NotificationEntity::new(&route, &stamped, ResponseFormat::Normalized).with_inline_join(plan.cursor()));
    assert_eq!(written["object"], json!(THREE));
    assert_eq!(nested_ids(&written["entity"]), THREE);
}

/// Clause 4.5.22.2 encodes a deleted `ListRelationship` as the array of one
/// NGSI-LD Null, and clause 4.5.0 makes a Relationship's Null the URI.
#[test]
fn a_notified_deletion_is_the_relationship_null() {
    let stamped = StampedInstants::default();
    let held = route(&["urn:ngsi-ld:null"]);
    assert_eq!(
        notified(NotificationEntity::new(&held, &stamped, ResponseFormat::Normalized)),
        json!({"type": "Relationship", "object": "urn:ngsi-ld:null"})
    );
    let three = route(&THREE);
    let reported = NotifiedChanges {
        held: HeldAttributes::Deleted,
        deleted: None,
        previous: None,
    };
    assert_eq!(
        notified(NotificationEntity::new(&three, &stamped, ResponseFormat::Normalized).with_changes(reported)),
        json!({"type": "Relationship", "object": "urn:ngsi-ld:null"})
    );
}
