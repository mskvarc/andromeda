use crate::{
    context::ResponseContext,
    entity_response::{EntityBody, EntityPayload},
    response::ResponseRenderer,
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
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::projection::attribute_iri_index::AttributeIriIndex;
use serde_json::json;
use std::{num::NonZeroU32, thread};

/// Links in the chain, each nested one level below the one naming it.
const CHAIN: u32 = 2_000;

/// The `index`th entity of the chain, naming the next one through `next`.
fn link(index: u32) -> Entity {
    serde_json::from_value(json!({
        "id": format!("urn:ngsi-ld:Link:{index}"),
        "type": "Link",
        "next": {"type": "Relationship", "object": format!("urn:ngsi-ld:Link:{}", index + 1)}
    }))
    .expect("a chain link decodes")
}

/// The body the root of the chain is written to under `join=inline` with a
/// `joinLevel` reaching its last entity.
fn written_chain() -> Vec<u8> {
    let root = link(0);
    let nodes: Vec<Entity> = (1..=CHAIN).map(link).collect();
    let RootEdges { ids, mut edges } = RootEdges::of::<TypedEntities>([&root]);
    for node in &nodes {
        TypedEntities::edges(node, &mut edges);
    }
    let graph = LinkedGraph {
        roots: ids,
        nodes: nodes.into_iter().map(|node| (node.id.clone(), node)).collect(), // clone: the graph keys its nodes by owned identifiers
        edges,
        failures: Vec::new(),
        depth: NonZeroU32::new(CHAIN).expect("a positive depth"),
    };
    let index = AttributeIriIndex::new();
    let JoinPlan::Inline(plan) = JoinPlan::<TypedEntities>::new(RetrievingJoin::Inline, &graph, MemberRule::new(ListInForce::Unrestricted, &index)) else {
        panic!("an inline join plans inline");
    };
    let body = EntityBody::new(&root, None).with_inline_join(plan.cursor());
    let mut written = Vec::new();
    ResponseRenderer::new(ResponseMediaType::Json, ResponseContext::Absent)
        .with_entities(EntityPayload::Single(body), None)
        .render_into(&mut written)
        .expect("the chain is written");
    written
}

/// Clause 4.5.23 sets no `joinLevel` cap, so the depth of an inline join is
/// bounded by the data alone: a chain nested far deeper than the thread's
/// stack holds frames for is still written whole.
#[test]
fn a_long_inline_chain_renders_on_a_nearly_exhausted_stack() {
    let written = thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(written_chain)
        .expect("the thread starts")
        .join()
        .expect("the chain is written without exhausting the stack");
    let nested = written.windows(br#""entity":"#.len()).filter(|window| window == br#""entity":"#).count();
    pretty_assertions::assert_eq!(nested, usize::try_from(CHAIN).expect("a small count"));
    let last = format!(r#""id":"urn:ngsi-ld:Link:{CHAIN}""#);
    assert!(written.windows(last.len()).any(|window| window == last.as_bytes()), "the last link is nested");
}
