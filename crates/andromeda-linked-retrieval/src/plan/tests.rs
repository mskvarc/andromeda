//! The plan over the Building graph: `A owner→P`, `A manager→P`,
//! `A locatedIn→C`, `P employer→K`.

use crate::{
    graph::{LinkedGraph, RootEdges},
    join::{RetrievingJoin, TraversalBounds},
    narrowing::{list::ListInForce, list_terms::ListTerms, member::MemberRule},
    plan::{InlineCursor, JoinPlan},
    test_graph::{FakeSource, Node, Nodes, graph, id, index, node, term},
    traversal::traverse,
};
use andromeda_ngsi_ld_query::projection::{attribute_iri_index::AttributeIriIndex, term::ProjectionTerm};
use pretty_assertions::assert_eq;
use std::{num::NonZeroU32, slice::from_ref};

fn building(depth: u32) -> LinkedGraph<Node> {
    graph(
        &[node("A", &[("owner", "P"), ("manager", "P"), ("locatedIn", "C")])],
        vec![
            node("P", &[("employer", "K")]).with_properties(&["name", "email"]),
            node("C", &[]).with_properties(&["name"]),
            node("K", &[]).with_properties(&["name", "size"]),
        ],
        depth,
    )
}

fn keys() -> AttributeIriIndex {
    index(&["owner", "manager", "locatedIn", "employer", "name", "email", "size", "next", "back"])
}

fn leaf(key: &str) -> ProjectionTerm {
    term(key, Vec::new())
}

fn flat_ids(plan: &JoinPlan<'_, Nodes>) -> Vec<String> {
    match plan {
        JoinPlan::Flat(flat) => flat.linked().map(|linked| linked.id.to_string()).collect(),
        JoinPlan::Inline(_) => panic!("a flat plan was asked for"),
    }
}

fn cursor<'p>(plan: &'p JoinPlan<'_, Nodes>) -> InlineCursor<'p, Nodes> {
    match plan {
        JoinPlan::Inline(inline) => inline.cursor(),
        JoinPlan::Flat(_) => panic!("an inline plan was asked for"),
    }
}

#[test]
fn a_flat_plan_lists_each_reachable_entity_once_in_encounter_order() {
    let (graph, index) = (building(1), keys());
    let plan = JoinPlan::<Nodes>::new(RetrievingJoin::Flat, &graph, MemberRule::new(ListInForce::Unrestricted, &index));
    assert_eq!(flat_ids(&plan), vec!["urn:ngsi-ld:T:P", "urn:ngsi-ld:T:C"]);
}

/// Each Relationship nests its own copy, narrowed by its own group (clause
/// 4.21).
#[test]
fn an_inline_plan_nests_a_copy_per_relationship() {
    let (graph, index) = (building(1), keys());
    let pick = [term("owner", vec![leaf("name")]), term("manager", vec![leaf("email")])];
    let plan = JoinPlan::<Nodes>::new(
        RetrievingJoin::Inline,
        &graph,
        MemberRule::new(ListInForce::Addressed(ListTerms::Pick(&pick)), &index),
    );
    let root = id("A");
    let open = cursor(&plan).open(&root);
    let p = id("P");
    let (_, under_owner) = open.nested("owner", &p).expect("P nests under owner");
    let (_, under_manager) = open.nested("manager", &p).expect("P nests under manager");
    assert_eq!(under_owner.rule().list(), ListInForce::Addressed(ListTerms::Pick(&[leaf("name")])));
    assert_eq!(under_manager.rule().list(), ListInForce::Addressed(ListTerms::Pick(&[leaf("email")])));
    assert!(open.nested("locatedIn", &id("C")).is_none(), "locatedIn is not returned");
}

/// Clause 4.5.23.1: a target open on the path stays a plain URI.
#[test]
fn an_inline_plan_stops_at_a_loop() {
    let graph = graph(
        &[node("A", &[("next", "B")])],
        vec![node("B", &[("next", "C")]), node("C", &[("back", "B")])],
        3,
    );
    let index = keys();
    let plan = JoinPlan::<Nodes>::new(RetrievingJoin::Inline, &graph, MemberRule::new(ListInForce::Unrestricted, &index));
    let (a, b, c) = (id("A"), id("B"), id("C"));
    let open_a = cursor(&plan).open(&a);
    let (_, at_b) = open_a.nested("next", &b).expect("B nests under A");
    let open_b = at_b.open(&b);
    let (_, at_c) = open_b.nested("next", &c).expect("C nests under B");
    assert!(at_c.open(&c).nested("back", &b).is_none(), "B is open on the path");
}

/// Clause 4.5.23 `containedBy`: what the client holds is never nested.
#[tokio::test]
async fn an_inline_plan_skips_contained_by_ids() {
    let root = node("A", &[("owner", "P")]);
    let source = FakeSource::local([node("P", &[]).with_properties(&["name"])]);
    let held = [id("P")];
    let graph = traverse(
        &source,
        RootEdges::of::<Nodes>(from_ref(&root)),
        &TraversalBounds::of(NonZeroU32::new(1), Some(&held)),
    )
    .await
    .expect("the traversal reads");
    let index = keys();
    let plan = JoinPlan::<Nodes>::new(RetrievingJoin::Inline, &graph, MemberRule::new(ListInForce::Unrestricted, &index));
    assert!(cursor(&plan).open(&root.id).nested("owner", &held[0]).is_none());
}

#[test]
fn an_inline_plan_keeps_object_order() {
    let graph = graph(&[node("A", &[("owner", "P"), ("owner", "Q")])], vec![node("P", &[]), node("Q", &[])], 1);
    let index = keys();
    let plan = JoinPlan::<Nodes>::new(RetrievingJoin::Inline, &graph, MemberRule::new(ListInForce::Unrestricted, &index));
    let root = id("A");
    let open = cursor(&plan).open(&root);
    let nested: Vec<String> = [id("P"), id("Q")]
        .iter()
        .filter_map(|target| open.nested("owner", target).map(|(node, _)| node.id.to_string()))
        .collect();
    assert_eq!(nested, vec!["urn:ngsi-ld:T:P", "urn:ngsi-ld:T:Q"]);
}

/// The group `manager{size}` names nothing `P` holds, so that copy is not
/// nested, while `owner`'s is.
#[test]
fn an_emptied_linked_entity_is_not_nested() {
    let (graph, index) = (building(1), keys());
    let pick = [term("owner", vec![leaf("name")]), term("manager", vec![leaf("size")])];
    let plan = JoinPlan::<Nodes>::new(
        RetrievingJoin::Inline,
        &graph,
        MemberRule::new(ListInForce::Addressed(ListTerms::Pick(&pick)), &index),
    );
    let (root, p) = (id("A"), id("P"));
    let open = cursor(&plan).open(&root);
    assert!(open.nested("owner", &p).is_some());
    assert!(open.nested("manager", &p).is_none());
}

/// Clause 4.21: in `[owner{name}, manager]` the bare `manager` addresses the
/// root alone, so the copy nested under it is whole.
#[test]
fn an_unaddressed_relationship_nests_its_target_whole() {
    let (graph, index) = (building(1), keys());
    let pick = [term("owner", vec![leaf("name")]), leaf("manager")];
    let plan = JoinPlan::<Nodes>::new(
        RetrievingJoin::Inline,
        &graph,
        MemberRule::new(ListInForce::Addressed(ListTerms::Pick(&pick)), &index),
    );
    let (root, p) = (id("A"), id("P"));
    let open = cursor(&plan).open(&root);
    let (_, under_manager) = open.nested("manager", &p).expect("P nests under manager");
    assert_eq!(under_manager.rule().list(), ListInForce::Unrestricted);
}

#[test]
fn an_emptied_linked_entity_is_not_appended() {
    let (graph, index) = (building(1), keys());
    let pick = [ProjectionTerm::Id, term("locatedIn", vec![leaf("size")])];
    let plan = JoinPlan::<Nodes>::new(
        RetrievingJoin::Flat,
        &graph,
        MemberRule::new(ListInForce::Addressed(ListTerms::Pick(&pick)), &index),
    );
    assert_eq!(flat_ids(&plan), Vec::<String>::new());
    assert_eq!(plan.linked_ids().len(), 0);
}

/// Clause 4.5.23.1: a target the local store holds joins whether or not its
/// Relationship carries an `objectType`.
#[tokio::test]
async fn a_local_target_without_object_type_is_planned() {
    let root = node("A", &[("owner", "P")]);
    let source = FakeSource::local([node("P", &[])]);
    let graph = traverse(&source, RootEdges::of::<Nodes>(from_ref(&root)), &TraversalBounds::of(None, None))
        .await
        .expect("the traversal reads");
    let index = keys();
    let plan = JoinPlan::<Nodes>::new(RetrievingJoin::Flat, &graph, MemberRule::new(ListInForce::Unrestricted, &index));
    assert_eq!(flat_ids(&plan), vec!["urn:ngsi-ld:T:P"]);
}
