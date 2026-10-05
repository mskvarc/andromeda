use crate::temporal::{inline_join::write_temporal_inline_join, temporal_evolutions::TemporalEvolutions};
use andromeda_linked_retrieval::{
    graph::{LinkedGraph, RootEdges},
    join::RetrievingJoin,
    narrowing::{list::ListInForce, member::MemberRule},
    plan::JoinPlan,
    representation::LinkedRepresentation,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::projection::{attribute_iri_index::AttributeIriIndex, term::ProjectionTerm};
use iri_rs::IriBuf;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::num::NonZeroU32;

fn iri(key: &str) -> IriBuf {
    IriBuf::new(format!("https://example.com/{key}")).expect("valid IRI")
}

fn index() -> AttributeIriIndex {
    let mut index = AttributeIriIndex::new();
    for key in ["owner", "manager", "name", "email", "parkedAt", "reading"] {
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

fn nested(key: &str, linked: Vec<ProjectionTerm>) -> ProjectionTerm {
    ProjectionTerm::Attribute { iri: iri(key), linked }
}

/// A temporal representation of one entity, carrying one plain observation
/// so it is distinguishable once nested.
fn linked(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Thing",
        "reading": [{"type": "Property", "value": 1, "observedAt": "2024-01-01T00:00:00Z"}]
    })
}

/// An entity whose `parkedAt` instances name `targets`, one instance each.
fn parked(id: &str, targets: &[&str]) -> Value {
    let instances: Vec<Value> = targets
        .iter()
        .map(|target| json!({"type": "Relationship", "object": target, "observedAt": "2024-01-01T00:00:00Z"}))
        .collect();
    json!({"id": id, "type": "Thing", "parkedAt": instances})
}

/// The graph a traversal `depth` levels deep from `root` would hold, every
/// node of `nodes` resolved.
fn graph(root: &Value, nodes: &[Value], depth: u32) -> LinkedGraph<Value> {
    let RootEdges { ids, mut edges } = RootEdges::of::<TemporalEvolutions>([root]);
    for node in nodes {
        TemporalEvolutions::edges(node, &mut edges);
    }
    LinkedGraph {
        roots: ids,
        nodes: nodes
            .iter()
            .map(|node| (TemporalEvolutions::identifier(node).expect("an identifier"), node.clone())) // clone: the graph owns its nodes
            .collect(),
        edges,
        failures: Vec::new(),
        depth: NonZeroU32::new(depth).expect("a positive depth"),
    }
}

/// `root` written with the inline plan over `nodes`, `depth` levels deep,
/// narrowed by `list`.
fn joined_under(root: Value, nodes: &[Value], depth: u32, list: ListInForce<'_>) -> Value {
    let index = index();
    let graph = graph(&root, nodes, depth);
    let JoinPlan::Inline(plan) = JoinPlan::<TemporalEvolutions>::new(RetrievingJoin::Inline, &graph, MemberRule::new(list, &index)) else {
        panic!("an inline join plans inline");
    };
    let mut root = root;
    write_temporal_inline_join(&mut root, plan.cursor());
    root
}

fn joined(root: Value, nodes: &[Value], depth: u32) -> Value {
    joined_under(root, nodes, depth, ListInForce::Unrestricted)
}

/// Clause 4.5.23 per instance: an Attribute that pointed at a different
/// entity at each observation nests a different entity under each instance.
#[test]
fn each_relationship_instance_nests_the_entity_that_instance_names() {
    let entity = joined(
        parked("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"]),
        &[linked("urn:ngsi-ld:T:B"), linked("urn:ngsi-ld:T:C")],
        1,
    );
    assert_eq!(entity["parkedAt"][0]["entity"]["id"], "urn:ngsi-ld:T:B");
    assert_eq!(entity["parkedAt"][1]["entity"]["id"], "urn:ngsi-ld:T:C");
    assert!(entity["parkedAt"][0]["entity"]["reading"].is_array());
}

/// Clause 4.5.23 leaves an unresolved Relationship a plain URI.
#[test]
fn an_unresolved_instance_is_left_untouched() {
    let entity = joined(
        parked("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:Missing"]),
        &[linked("urn:ngsi-ld:T:B")],
        1,
    );
    assert!(entity["parkedAt"][0].get("entity").is_some());
    assert!(entity["parkedAt"][1].get("entity").is_none());
    assert_eq!(entity["parkedAt"][1]["object"], "urn:ngsi-ld:T:Missing");
}

#[test]
fn a_single_instance_rendered_as_a_bare_object_is_expanded() {
    let root = json!({
        "id": "urn:ngsi-ld:T:A",
        "type": "Thing",
        "parkedAt": {"type": "Relationship", "object": "urn:ngsi-ld:T:B", "observedAt": "2024-01-01T00:00:00Z"}
    });
    let entity = joined(root, &[linked("urn:ngsi-ld:T:B")], 1);
    assert_eq!(entity["parkedAt"]["entity"]["id"], "urn:ngsi-ld:T:B");
}

/// Clause 4.5.3.2: an instance naming several targets nests every one of
/// them, in order, and an array of one answers with an array of one.
#[test]
fn an_object_array_nests_an_entity_array_in_order() {
    let root = json!({
        "id": "urn:ngsi-ld:T:A",
        "type": "Thing",
        "passengers": [{"type": "Relationship", "object": ["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"]}],
        "driver": [{"type": "Relationship", "object": ["urn:ngsi-ld:T:B"]}]
    });
    let entity = joined(root, &[linked("urn:ngsi-ld:T:B"), linked("urn:ngsi-ld:T:C")], 1);
    let passengers: Vec<&Value> = entity["passengers"][0]["entity"]
        .as_array()
        .expect("an array")
        .iter()
        .map(|nested| &nested["id"])
        .collect();
    assert_eq!(passengers, vec!["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"]);
    assert_eq!(entity["driver"][0]["entity"].as_array().expect("an array").len(), 1);
}

/// Clauses 4.5.22.2 and 4.5.23.2: both spellings of an `objectList` nest one
/// entry per resolved member, in list order.
#[test]
fn an_object_list_nests_the_entities_that_resolved_in_list_order() {
    for object_list in [
        json!(["urn:ngsi-ld:T:C", "urn:ngsi-ld:T:Missing", "urn:ngsi-ld:T:B"]),
        json!([{"object": "urn:ngsi-ld:T:C"}, {"object": "urn:ngsi-ld:T:Missing"}, {"object": "urn:ngsi-ld:T:B"}]),
    ] {
        let root = json!({"id": "urn:ngsi-ld:T:A", "type": "Thing", "hasParts": [{"type": "ListRelationship", "objectList": object_list}]});
        let entity = joined(root, &[linked("urn:ngsi-ld:T:B"), linked("urn:ngsi-ld:T:C")], 1);
        let nested: Vec<&Value> = entity["hasParts"][0]["entityList"]
            .as_array()
            .expect("an array")
            .iter()
            .map(|item| &item["id"])
            .collect();
        assert_eq!(nested, vec!["urn:ngsi-ld:T:C", "urn:ngsi-ld:T:B"]);
    }
}

#[test]
fn a_second_level_nests_inside_the_first() {
    let entity = joined(
        parked("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"]),
        &[parked("urn:ngsi-ld:T:B", &["urn:ngsi-ld:T:C"]), linked("urn:ngsi-ld:T:C")],
        2,
    );
    assert_eq!(entity["parkedAt"][0]["entity"]["parkedAt"][0]["entity"]["id"], "urn:ngsi-ld:T:C");
}

/// A level the traversal did not walk to is not nested, even when the entity
/// is held for another path.
#[test]
fn a_level_beyond_the_join_level_stays_a_uri() {
    let entity = joined(
        parked("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"]),
        &[parked("urn:ngsi-ld:T:B", &["urn:ngsi-ld:T:C"]), linked("urn:ngsi-ld:T:C")],
        1,
    );
    assert_eq!(entity["parkedAt"][0]["entity"]["id"], "urn:ngsi-ld:T:B");
    assert!(entity["parkedAt"][0]["entity"]["parkedAt"][0].get("entity").is_none());
}

/// Clause 4.5.23.1: a target open on the path from the root stays a plain
/// URI, so a cycle terminates.
#[test]
fn a_cycle_stops_at_the_entity_already_open() {
    let entity = joined(
        parked("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:A"]),
        &[parked("urn:ngsi-ld:T:B", &["urn:ngsi-ld:T:A"])],
        5,
    );
    assert_eq!(entity["parkedAt"][0]["entity"]["id"], "urn:ngsi-ld:T:B");
    assert!(entity["parkedAt"][0]["entity"]["parkedAt"][0].get("entity").is_none());
    assert!(entity["parkedAt"][1].get("entity").is_none());
}

/// Decision A: every Relationship naming one target nests its own copy,
/// narrowed by the group that Relationship's term gives it.
#[test]
fn each_relationship_nests_its_own_narrowed_copy() {
    let root = json!({
        "id": "urn:ngsi-ld:B:1",
        "type": "Building",
        "owner": [{"type": "Relationship", "object": "urn:ngsi-ld:P:1"}],
        "manager": [{"type": "Relationship", "object": "urn:ngsi-ld:P:1"}]
    });
    let person = json!({
        "id": "urn:ngsi-ld:P:1",
        "type": "Person",
        "name": [{"type": "Property", "value": "Ada"}],
        "email": [{"type": "Property", "value": "ada@example.org"}]
    });
    let pick = [nested("owner", vec![leaf("name")]), nested("manager", vec![leaf("email")])];
    let entity = joined_under(root, &[person], 1, ListInForce::Pick(&pick));
    let owner = entity["owner"][0]["entity"].as_object().expect("an owner copy");
    let manager = entity["manager"][0]["entity"].as_object().expect("a manager copy");
    assert_eq!(owner.keys().collect::<Vec<_>>(), vec!["name"]);
    assert_eq!(manager.keys().collect::<Vec<_>>(), vec!["email"]);
}

/// D9: a Linked Entity its list leaves no member is not nested, and its
/// Relationship keeps the `object` naming it.
#[test]
fn an_emptied_linked_entity_is_not_nested() {
    let root = json!({
        "id": "urn:ngsi-ld:B:1",
        "type": "Building",
        "owner": [{"type": "Relationship", "object": "urn:ngsi-ld:P:1"}],
        "manager": [{"type": "Relationship", "object": "urn:ngsi-ld:P:1"}]
    });
    let person = json!({"id": "urn:ngsi-ld:P:1", "type": "Person", "name": [{"type": "Property", "value": "Ada"}], "email": [{"type": "Property", "value": "ada@example.org"}]});
    let pick = [nested("owner", vec![leaf("name")]), leaf("manager")];
    let entity = joined_under(root, &[person], 1, ListInForce::Pick(&pick));
    assert_eq!(entity["owner"][0]["entity"]["name"][0]["value"], "Ada");
    assert!(entity["manager"][0].get("entity").is_none());
    assert_eq!(entity["manager"][0]["object"], "urn:ngsi-ld:P:1");
}

/// Clause 4.21: a root whose list removes `id` is written without it, and
/// its returned Relationships still nest their targets.
#[test]
fn a_root_written_without_its_id_still_nests() {
    let root = parked("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"]);
    let graph = graph(&root, &[linked("urn:ngsi-ld:T:B")], 1);
    let index = index();
    let pick = [leaf("parkedAt"), leaf("reading")];
    let JoinPlan::Inline(plan) = JoinPlan::<TemporalEvolutions>::new(RetrievingJoin::Inline, &graph, MemberRule::new(ListInForce::Pick(&pick), &index)) else {
        panic!("an inline join plans inline");
    };
    let mut unnamed = json!({"parkedAt": root["parkedAt"].clone()}); // clone: the written root holds its own instances

    write_temporal_inline_join(&mut unnamed, plan.cursor());

    assert_eq!(
        unnamed["parkedAt"][0]["entity"],
        json!({"reading": [{"type": "Property", "value": 1, "observedAt": "2024-01-01T00:00:00Z"}]})
    );
}
