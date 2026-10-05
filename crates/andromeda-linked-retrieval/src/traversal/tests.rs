//! The traversal, run over a recording source.

use crate::{
    graph::{LinkedGraph, RootEdges},
    join::TraversalBounds,
    test_graph::{Call, FakeSource, Node, Nodes, RemoteAnswer, id, node},
    traversal::traverse,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_representation::linked_resolution_failure::LinkedResolutionFailureKind;
use pretty_assertions::assert_eq;
use std::{num::NonZeroU32, slice::from_ref};

fn level(depth: u32) -> Option<NonZeroU32> {
    NonZeroU32::new(depth)
}

async fn walk(source: &FakeSource, root: &Node, bounds: &TraversalBounds) -> LinkedGraph<Node> {
    traverse(source, RootEdges::of::<Nodes>(from_ref(root)), bounds)
        .await
        .expect("the traversal reads")
}

fn resolved(graph: &LinkedGraph<Node>) -> Vec<&str> {
    graph.nodes.keys().map(Identifier::as_str).collect()
}

fn ids(raw: &[&str]) -> Vec<Identifier> {
    raw.iter().map(|raw| id(raw)).collect()
}

#[tokio::test]
async fn depth_one_resolves_only_the_direct_targets() {
    let source = FakeSource::local([node("B", &[("next", "C")]), node("C", &[])]);
    let graph = walk(&source, &node("A", &[("next", "B")]), &TraversalBounds::of(level(1), None)).await;
    assert_eq!(resolved(&graph), vec!["urn:ngsi-ld:T:B"]);
}

#[tokio::test]
async fn depth_two_resolves_level_by_level_in_encounter_order() {
    let source = FakeSource::local([node("B", &[("next", "D")]), node("C", &[]), node("D", &[])]);
    let graph = walk(&source, &node("A", &[("first", "B"), ("second", "C")]), &TraversalBounds::of(level(2), None)).await;
    assert_eq!(resolved(&graph), vec!["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C", "urn:ngsi-ld:T:D"]);
}

#[tokio::test]
async fn a_cycle_back_to_a_root_terminates() {
    let source = FakeSource::local([node("B", &[("back", "A")])]);
    let graph = walk(&source, &node("A", &[("next", "B")]), &TraversalBounds::of(level(5), None)).await;
    assert_eq!(resolved(&graph), vec!["urn:ngsi-ld:T:B"]);
}

#[tokio::test]
async fn a_self_reference_terminates() {
    let source = FakeSource::local([node("B", &[("self", "B")])]);
    let graph = walk(&source, &node("A", &[("next", "B")]), &TraversalBounds::of(level(3), None)).await;
    assert_eq!(resolved(&graph), vec!["urn:ngsi-ld:T:B"]);
}

/// Clause 4.5.23 `containedBy`: what the client already holds is neither read
/// nor returned.
#[tokio::test]
async fn contained_by_ids_are_neither_read_nor_returned() {
    let source = FakeSource::local([node("B", &[])]);
    let held = ids(&["B"]);
    let graph = walk(&source, &node("A", &[("next", "B")]), &TraversalBounds::of(level(2), Some(&held))).await;
    assert_eq!(resolved(&graph), Vec::<&str>::new());
    assert_eq!(*source.calls(), Vec::new());
}

/// Clause 4.5.23.1: an annotated target the local store does not hold is asked
/// of the remainder, with the levels still to resolve and the ids already held.
#[tokio::test]
async fn an_eligible_local_miss_is_fetched_remotely() {
    let source = FakeSource::local([]).federated([("B", RemoteAnswer::Nodes(vec![node("B", &[])]))]);
    let graph = walk(&source, &node("A", &[("next", "B")]).annotated(), &TraversalBounds::of(level(3), None)).await;
    assert_eq!(resolved(&graph), vec!["urn:ngsi-ld:T:B"]);
    assert_eq!(
        source.fetches(),
        vec![Call::Fetch {
            id: id("B"),
            depth: 3,
            held: ids(&["A"]),
        }]
    );
}

/// Clause 4.5.23.1: an unannotated target stays a plain URI.
#[tokio::test]
async fn a_local_only_miss_is_left_unresolved_silently() {
    let source = FakeSource::local([]).federated([("B", RemoteAnswer::Nodes(vec![node("B", &[])]))]);
    let graph = walk(&source, &node("A", &[("next", "B")]), &TraversalBounds::of(level(1), None)).await;
    assert_eq!(resolved(&graph), Vec::<&str>::new());
    assert!(graph.failures.is_empty(), "{:?}", graph.failures);
    assert_eq!(source.fetches(), Vec::new());
}

#[tokio::test]
async fn a_remote_error_becomes_an_unreachable_failure() {
    let source = FakeSource::local([]).federated([("B", RemoteAnswer::Unreachable)]);
    let graph = walk(&source, &node("A", &[("next", "B")]).annotated(), &TraversalBounds::of(level(1), None)).await;
    let failures: Vec<(&str, LinkedResolutionFailureKind)> = graph.failures.iter().map(|failure| (failure.id.as_str(), failure.kind)).collect();
    assert_eq!(failures, vec![("urn:ngsi-ld:T:B", LinkedResolutionFailureKind::Unreachable)]);
}

/// The sub-graph a holder resolved around a candidate is kept, and not walked
/// again: walking it would spend levels twice.
#[tokio::test]
async fn a_remote_sub_tree_is_kept_but_not_walked_again() {
    let source = FakeSource::local([]).federated([(
        "B",
        RemoteAnswer::Nodes(vec![node("B", &[("next", "C")]).annotated(), node("C", &[("next", "D")]).annotated()]),
    )]);
    let graph = walk(&source, &node("A", &[("next", "B")]).annotated(), &TraversalBounds::of(level(2), None)).await;
    assert_eq!(resolved(&graph), vec!["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"]);
    assert!(
        !source.calls().contains(&Call::Read(ids(&["C"]))),
        "the holder's extra entity is never read: {:?}",
        source.calls()
    );
    assert_eq!(source.fetches().len(), 1);
}

#[tokio::test]
async fn a_remote_answer_holding_an_already_held_id_is_dropped() {
    let source = FakeSource::local([]).federated([("B", RemoteAnswer::Nodes(vec![node("B", &[]), node("A", &[])]))]);
    let graph = walk(&source, &node("A", &[("next", "B")]).annotated(), &TraversalBounds::of(level(2), None)).await;
    assert_eq!(resolved(&graph), vec!["urn:ngsi-ld:T:B"]);
}

#[tokio::test]
async fn a_non_federating_source_never_fetches_remotely() {
    let source = FakeSource::local([])
        .federated([("B", RemoteAnswer::Nodes(vec![node("B", &[])]))])
        .without_federation();
    let graph = walk(&source, &node("A", &[("next", "B")]).annotated(), &TraversalBounds::of(level(1), None)).await;
    assert_eq!(resolved(&graph), Vec::<&str>::new());
    assert_eq!(source.fetches(), Vec::new());
}

/// Within one level the local store's answers come first, whatever order the
/// candidates were named in.
#[tokio::test]
async fn local_answers_precede_remote_answers_within_a_level() {
    let source = FakeSource::local([node("B", &[]), node("D", &[])]).federated([("C", RemoteAnswer::Nodes(vec![node("C", &[])]))]);
    let root = node("A", &[("first", "B"), ("second", "C"), ("third", "D")]).annotated();
    let graph = walk(&source, &root, &TraversalBounds::of(level(1), None)).await;
    assert_eq!(resolved(&graph), vec!["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:D", "urn:ngsi-ld:T:C"]);
}

#[tokio::test]
async fn every_edge_records_its_entity() {
    let source = FakeSource::local([node("B", &[("next", "C")]), node("C", &[])]);
    let graph = walk(&source, &node("A", &[("next", "B")]), &TraversalBounds::of(level(2), None)).await;
    let edges: Vec<(&str, &str)> = graph.edges.iter().map(|edge| (edge.from.as_str(), edge.target.as_str())).collect();
    assert_eq!(edges, vec![("urn:ngsi-ld:T:A", "urn:ngsi-ld:T:B"), ("urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C")]);
}

#[tokio::test]
async fn a_subgraph_holder_is_asked_for_the_remaining_depth() {
    let source = FakeSource::local([]).federated([("B", RemoteAnswer::Nodes(vec![node("B", &[("next", "C")]), node("C", &[])]))]);
    let graph = walk(&source, &node("A", &[("next", "B")]).annotated(), &TraversalBounds::of(level(3), None)).await;
    assert_eq!(resolved(&graph), vec!["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"]);
    let depths: Vec<u32> = source
        .fetches()
        .iter()
        .filter_map(|call| match call {
            Call::Fetch { depth, .. } => Some(*depth),
            Call::Read(_) => None,
        })
        .collect();
    assert_eq!(depths, vec![3]);
}

/// A holder that answers for one entity only is asked for that entity, and its
/// Relationships are walked here.
#[tokio::test]
async fn a_single_entity_holder_is_asked_for_one_level_and_walked_here() {
    let source = FakeSource::local([])
        .federated([
            ("B", RemoteAnswer::Nodes(vec![node("B", &[("next", "C")]).annotated()])),
            ("C", RemoteAnswer::Nodes(vec![node("C", &[])])),
        ])
        .single_entity_holders();
    let graph = walk(&source, &node("A", &[("next", "B")]).annotated(), &TraversalBounds::of(level(3), None)).await;
    assert_eq!(resolved(&graph), vec!["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"]);
    let fetched: Vec<(Identifier, u32)> = source
        .fetches()
        .into_iter()
        .filter_map(|call| match call {
            Call::Fetch { id, depth, .. } => Some((id, depth)),
            Call::Read(_) => None,
        })
        .collect();
    assert_eq!(fetched, vec![(id("B"), 1), (id("C"), 1)]);
}

/// The edges a level read reports are walked even when the node's own
/// spelling no longer shows them.
#[tokio::test]
async fn the_level_read_edges_are_used_not_the_rendered_nodes() {
    let source = FakeSource::local([node("B", &[("next", "C")]).hiding_its_links(), node("C", &[])]);
    let graph = walk(&source, &node("A", &[("next", "B")]), &TraversalBounds::of(level(2), None)).await;
    assert_eq!(resolved(&graph), vec!["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"]);
}
