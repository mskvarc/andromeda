use crate::{
    context::ResponseContext,
    response::ResponseRenderer,
    structural_projection::StructuralProjection,
    temporal::{
        temporal_evolutions::TemporalEvolutions,
        temporal_response::{TemporalPayload, TemporalRepresentation, TemporalRoot},
    },
};
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_json_literal::piece::JsonPiece;
use andromeda_linked_retrieval::{
    graph::{LinkedGraph, RootEdges},
    join::RetrievingJoin,
    narrowing::{list::ListInForce, list_terms::ListTerms, member::MemberRule},
    plan::JoinPlan,
    representation::LinkedRepresentation,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, temporal_format::TemporalFormat, time_prop::TimeProp};
use andromeda_ngsi_ld_model::temporal::{temporal_body::TemporalBody, temporal_entity::TemporalEntity};
use andromeda_ngsi_ld_query::projection::{attribute_iri_index::AttributeIriIndex, term::ProjectionTerm};
use iri_rs::IriBuf;
use pretty_assertions::assert_eq;
use serde_json::{Value, json, value::RawValue};
use std::{num::NonZeroU32, thread};

const NORMALIZED: TemporalRepresentation = TemporalRepresentation {
    format: TemporalFormat::Normalized,
    timeproperty: TimeProp::Observed,
};

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

/// The evolution of the entity `id` holding `members`.
fn thing(id: &str, members: Value) -> TemporalBody {
    let mut document = members;
    document["id"] = json!(id);
    document["type"] = json!("Thing");
    let raw = RawValue::from_string(document.to_string()).expect("raw JSON");
    TemporalBody::Evolution(TemporalEntity::from_node(JsonPiece::new(&raw)).expect("an evolution"))
}

/// A temporal representation of one entity, carrying one plain observation so
/// it is distinguishable once nested.
fn linked(id: &str) -> TemporalBody {
    thing(id, json!({"reading": [{"type": "Property", "value": 1, "observedAt": "2024-01-01T00:00:00Z"}]}))
}

/// An entity whose `parkedAt` instances name `targets`, one instance each.
fn parked(id: &str, targets: &[&str]) -> TemporalBody {
    let instances: Vec<Value> = targets
        .iter()
        .map(|target| json!({"type": "Relationship", "object": target, "observedAt": "2024-01-01T00:00:00Z"}))
        .collect();
    thing(id, json!({"parkedAt": instances}))
}

/// The graph a traversal `depth` levels deep from `root` would hold, every
/// node of `nodes` resolved.
fn graph(root: &TemporalBody, nodes: Vec<TemporalBody>, depth: u32) -> LinkedGraph<TemporalBody> {
    let RootEdges { ids, mut edges } = RootEdges::of::<TemporalEvolutions>([root]);
    for node in &nodes {
        TemporalEvolutions::edges(node, &mut edges);
    }
    LinkedGraph {
        roots: ids,
        nodes: nodes.into_iter().map(|node| (node.id().clone(), node)).collect(), // clone: the graph keys its nodes by owned identifiers
        edges,
        failures: Vec::new(),
        depth: NonZeroU32::new(depth).expect("a positive depth"),
    }
}

/// `root` written with the inline plan over `nodes`, `depth` levels deep,
/// narrowed by `list`.
fn joined_under(root: &TemporalBody, nodes: Vec<TemporalBody>, depth: u32, list: ListInForce<'_>) -> Vec<u8> {
    joined_narrowed(root, nodes, depth, list, None)
}

/// As [`joined_under`], the root itself narrowed by `projection`.
fn joined_narrowed(root: &TemporalBody, nodes: Vec<TemporalBody>, depth: u32, list: ListInForce<'_>, projection: Option<StructuralProjection<'_>>) -> Vec<u8> {
    let index = index();
    let graph = graph(root, nodes, depth);
    let JoinPlan::Inline(plan) = JoinPlan::<TemporalEvolutions>::new(RetrievingJoin::Inline, &graph, MemberRule::new(list, &index)) else {
        panic!("an inline join plans inline");
    };
    let mut written = Vec::new();
    ResponseRenderer::new(ResponseMediaType::Json, ResponseContext::Absent)
        .with_temporal(
            TemporalPayload::Single(TemporalRoot::new(root, projection).with_inline_join(plan.cursor())),
            NORMALIZED,
            None,
        )
        .render_into(&mut written)
        .expect("the join is written");
    written
}

fn parsed(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("the body is JSON")
}

fn joined(root: &TemporalBody, nodes: Vec<TemporalBody>, depth: u32) -> Value {
    parsed(&joined_under(root, nodes, depth, ListInForce::Unrestricted))
}

/// Clause 4.5.23 per instance: an Attribute that pointed at a different entity
/// at each observation nests a different entity under each instance.
#[test]
fn inline_join_nests_the_linked_evolution_per_instance() {
    let root = parked("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"]);
    let entity = joined(&root, vec![linked("urn:ngsi-ld:T:B"), linked("urn:ngsi-ld:T:C")], 1);
    assert_eq!(entity["parkedAt"][0]["entity"]["id"], "urn:ngsi-ld:T:B");
    assert_eq!(entity["parkedAt"][1]["entity"]["id"], "urn:ngsi-ld:T:C");
    assert!(entity["parkedAt"][0]["entity"]["reading"].is_array());
}

/// Clause 4.5.23 leaves an unresolved Relationship a plain URI.
#[test]
fn an_unresolved_instance_is_left_untouched() {
    let root = parked("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:Missing"]);
    let entity = joined(&root, vec![linked("urn:ngsi-ld:T:B")], 1);
    assert!(entity["parkedAt"][0].get("entity").is_some());
    assert!(entity["parkedAt"][1].get("entity").is_none());
    assert_eq!(entity["parkedAt"][1]["object"], "urn:ngsi-ld:T:Missing");
}

/// Clause 4.5.3.2: an instance naming several targets nests every one of them,
/// in order, and an array of one answers with an array of one.
#[test]
fn an_object_array_nests_an_entity_array_in_order() {
    let root = thing(
        "urn:ngsi-ld:T:A",
        json!({
            "passengers": [{"type": "Relationship", "object": ["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"]}],
            "driver": [{"type": "Relationship", "object": ["urn:ngsi-ld:T:B"]}]
        }),
    );
    let entity = joined(&root, vec![linked("urn:ngsi-ld:T:B"), linked("urn:ngsi-ld:T:C")], 1);
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
        let root = thing(
            "urn:ngsi-ld:T:A",
            json!({"hasParts": [{"type": "ListRelationship", "objectList": object_list}]}),
        );
        let entity = joined(&root, vec![linked("urn:ngsi-ld:T:B"), linked("urn:ngsi-ld:T:C")], 1);
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
    let root = parked("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"]);
    let entity = joined(&root, vec![parked("urn:ngsi-ld:T:B", &["urn:ngsi-ld:T:C"]), linked("urn:ngsi-ld:T:C")], 2);
    assert_eq!(entity["parkedAt"][0]["entity"]["parkedAt"][0]["entity"]["id"], "urn:ngsi-ld:T:C");
}

/// A level the traversal did not walk to is not nested, even when the entity
/// is held for another path.
#[test]
fn a_level_beyond_the_join_level_stays_a_uri() {
    let root = parked("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"]);
    let entity = joined(&root, vec![parked("urn:ngsi-ld:T:B", &["urn:ngsi-ld:T:C"]), linked("urn:ngsi-ld:T:C")], 1);
    assert_eq!(entity["parkedAt"][0]["entity"]["id"], "urn:ngsi-ld:T:B");
    assert!(entity["parkedAt"][0]["entity"]["parkedAt"][0].get("entity").is_none());
}

/// Clause 4.5.23.1: a target open on the path from the root stays a plain URI,
/// so a cycle terminates.
#[test]
fn inline_join_cuts_cycles_on_the_path() {
    let root = parked("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:A"]);
    let entity = joined(&root, vec![parked("urn:ngsi-ld:T:B", &["urn:ngsi-ld:T:A"])], 5);
    assert_eq!(entity["parkedAt"][0]["entity"]["id"], "urn:ngsi-ld:T:B");
    assert!(entity["parkedAt"][0]["entity"]["parkedAt"][0].get("entity").is_none());
    assert!(entity["parkedAt"][1].get("entity").is_none());
}

/// Every Relationship naming one target nests its own copy,
/// narrowed by the group that Relationship's term gives it.
#[test]
fn nested_projection_narrows_by_expanded_name() {
    let root = thing(
        "urn:ngsi-ld:B:1",
        json!({
            "owner": [{"type": "Relationship", "object": "urn:ngsi-ld:P:1"}],
            "manager": [{"type": "Relationship", "object": "urn:ngsi-ld:P:1"}]
        }),
    );
    let person = thing(
        "urn:ngsi-ld:P:1",
        json!({"name": [{"type": "Property", "value": "Ada"}], "email": [{"type": "Property", "value": "ada@example.org"}]}),
    );
    let pick = [nested("owner", vec![leaf("name")]), nested("manager", vec![leaf("email")])];
    let entity = parsed(&joined_under(&root, vec![person], 1, ListInForce::Addressed(ListTerms::Pick(&pick))));
    let owner = entity["owner"][0]["entity"].as_object().expect("an owner copy");
    let manager = entity["manager"][0]["entity"].as_object().expect("a manager copy");
    assert_eq!(owner.keys().collect::<Vec<_>>(), vec!["name"]);
    assert_eq!(manager.keys().collect::<Vec<_>>(), vec!["email"]);
}

/// A Linked Entity its list leaves no member is not nested, and its
/// Relationship keeps the `object` naming it.
#[test]
fn an_emptied_linked_entity_is_not_nested() {
    let root = thing(
        "urn:ngsi-ld:B:1",
        json!({
            "owner": [{"type": "Relationship", "object": "urn:ngsi-ld:P:1"}],
            "manager": [{"type": "Relationship", "object": "urn:ngsi-ld:P:1"}]
        }),
    );
    let person = thing(
        "urn:ngsi-ld:P:1",
        json!({"name": [{"type": "Property", "value": "Ada"}], "email": [{"type": "Property", "value": "ada@example.org"}]}),
    );
    let pick = [nested("owner", vec![leaf("name")]), nested("manager", vec![leaf("size")])];
    let entity = parsed(&joined_under(&root, vec![person], 1, ListInForce::Addressed(ListTerms::Pick(&pick))));
    assert_eq!(entity["owner"][0]["entity"]["name"][0]["value"], "Ada");
    assert!(entity["manager"][0].get("entity").is_none());
    assert_eq!(entity["manager"][0]["object"], "urn:ngsi-ld:P:1");
}

/// Clause 4.21: once the list holds a Linked Entity Term, its plain terms
/// address the root alone, so the copy nested under a Relationship no group
/// names is whole.
#[test]
fn a_relationship_no_group_addresses_nests_its_target_whole() {
    let root = thing(
        "urn:ngsi-ld:B:1",
        json!({
            "owner": [{"type": "Relationship", "object": "urn:ngsi-ld:P:1"}],
            "manager": [{"type": "Relationship", "object": "urn:ngsi-ld:P:1"}]
        }),
    );
    let person = thing(
        "urn:ngsi-ld:P:1",
        json!({"name": [{"type": "Property", "value": "Ada"}], "email": [{"type": "Property", "value": "ada@example.org"}]}),
    );
    let pick = [nested("owner", vec![leaf("name")]), leaf("manager")];
    let entity = parsed(&joined_under(&root, vec![person], 1, ListInForce::Addressed(ListTerms::Pick(&pick))));
    let owner = entity["owner"][0]["entity"].as_object().expect("an owner copy");
    let manager = entity["manager"][0]["entity"].as_object().expect("a manager copy");
    assert_eq!(owner.keys().collect::<Vec<_>>(), vec!["name"]);
    for member in ["id", "type", "name", "email"] {
        assert!(manager.contains_key(member), "the manager copy keeps `{member}`: {manager:?}");
    }
}

/// Clause 4.21: a root whose list removes `id` is written without it, and its
/// returned Relationships still nest their targets.
#[test]
fn a_root_written_without_its_id_still_nests() {
    let root = parked("urn:ngsi-ld:T:A", &["urn:ngsi-ld:T:B"]);
    let pick = [leaf("parkedAt"), leaf("reading")];
    let entity = parsed(&joined_narrowed(
        &root,
        vec![linked("urn:ngsi-ld:T:B")],
        1,
        ListInForce::Payload(ListTerms::Pick(&pick)),
        Some(StructuralProjection::new(&pick, &[])),
    ));
    assert!(entity.get("id").is_none(), "{entity}");
    assert_eq!(
        entity["parkedAt"][0]["entity"],
        json!({"reading": [{"type": "Property", "value": 1, "observedAt": "2024-01-01T00:00:00Z"}]})
    );
}

/// Links in the chain the deep inline test nests.
const CHAIN: u32 = 2_000;

/// The temporal representation of the `index`th link of the chain, naming the
/// next one.
fn chain_link(index: u32) -> TemporalBody {
    parked(&format!("urn:ngsi-ld:T:{index}"), &[&format!("urn:ngsi-ld:T:{}", index + 1)])
}

/// Clause 4.5.23 sets no `joinLevel` cap, so the depth of an inline join is
/// bounded by the data alone: a chain nested far deeper than the thread's stack
/// holds frames for is still written, and the typed model it is written from
/// released, whole.
#[test]
fn a_2000_level_joined_temporal_body_is_written_and_released_on_a_256_kib_thread() {
    let written = thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let root = chain_link(0);
            let nodes: Vec<TemporalBody> = (1..=CHAIN).map(chain_link).collect();
            joined_under(&root, nodes, CHAIN, ListInForce::Unrestricted)
        })
        .expect("the thread starts")
        .join()
        .expect("the chain is written and released without exhausting the stack");
    let nested = written.windows(br#""entity":"#.len()).filter(|window| window == br#""entity":"#).count();
    assert_eq!(nested, usize::try_from(CHAIN).expect("a small count"));
    let last = format!(r#""id":"urn:ngsi-ld:T:{CHAIN}""#);
    assert!(written.windows(last.len()).any(|window| window == last.as_bytes()), "the last link is nested");
}
