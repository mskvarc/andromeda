//! The first-encounter walk over the Building graph: `A owner→P`,
//! `A manager→P`, `A locatedIn→C`, `P employer→K`, two levels deep.

use crate::{
    graph::LinkedGraph,
    narrowing::{encounter::FlatEncounters, list::ListInForce, member::MemberRule},
    test_graph::{Node, Nodes, graph, index, node, term},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::projection::{attribute_iri_index::AttributeIriIndex, term::ProjectionTerm};
use pretty_assertions::assert_eq;
use std::num::NonZeroU32;

fn building() -> LinkedGraph<Node> {
    graph(
        &[node("A", &[("owner", "P"), ("manager", "P"), ("locatedIn", "C")])],
        vec![
            node("P", &[("employer", "K")]).with_properties(&["name", "email"]),
            node("C", &[]).with_properties(&["name"]),
            node("K", &[]).with_properties(&["name", "size"]),
        ],
        2,
    )
}

fn keys() -> AttributeIriIndex {
    index(&["owner", "manager", "locatedIn", "employer", "name", "email", "size", "linked", "x"])
}

fn written<'g>(graph: &'g LinkedGraph<Node>, list: ListInForce<'g>, index: &'g AttributeIriIndex) -> Vec<(&'g str, ListInForce<'g>)> {
    FlatEncounters::walk::<Nodes>(graph, MemberRule::new(list, index))
        .written()
        .map(|(id, rule)| (id.as_str(), rule.list()))
        .collect()
}

fn leaf(key: &str) -> ProjectionTerm {
    term(key, Vec::new())
}

/// Clause 4.5.23.3: the first encounter, through `owner`, decides `P`.
#[test]
fn the_first_encounter_decides_the_list() {
    let (graph, index) = (building(), keys());
    let pick = [term("owner", vec![leaf("name")]), term("manager", vec![leaf("email")])];
    let name = [leaf("name")];
    assert_eq!(
        written(&graph, ListInForce::Pick(&pick), &index),
        vec![("urn:ngsi-ld:T:P", ListInForce::Pick(&name))]
    );
}

/// Clause 5.7.1.5: only the returned Attributes bring their targets.
#[test]
fn a_relationship_the_list_removes_reaches_nothing() {
    let (graph, index) = (building(), keys());
    let pick = [ProjectionTerm::Id, term("owner", vec![leaf("name")])];
    let reached: Vec<&str> = written(&graph, ListInForce::Pick(&pick), &index).into_iter().map(|(id, _)| id).collect();
    assert_eq!(reached, vec!["urn:ngsi-ld:T:P"]);
}

#[test]
fn an_inherited_group_reaches_the_second_level() {
    let (graph, index) = (building(), keys());
    let pick = [ProjectionTerm::Id, term("owner", vec![leaf("name"), leaf("employer")])];
    let group = [leaf("name"), leaf("employer")];
    assert_eq!(
        written(&graph, ListInForce::Pick(&pick), &index),
        vec![("urn:ngsi-ld:T:P", ListInForce::Pick(&group)), ("urn:ngsi-ld:T:K", ListInForce::Pick(&group))]
    );
}

/// Clause 5.7.1.5: a plain list "applies to all Entities in the payload".
#[test]
fn a_bare_term_passes_its_list_down() {
    let (graph, index) = (building(), keys());
    let pick = [ProjectionTerm::Id, leaf("name"), leaf("owner")];
    assert_eq!(
        written(&graph, ListInForce::Pick(&pick), &index),
        vec![("urn:ngsi-ld:T:P", ListInForce::Pick(&pick))]
    );
}

#[test]
fn an_omit_passes_down_until_overridden() {
    let (graph, index) = (building(), keys());
    let omit = [leaf("email")];
    assert_eq!(
        written(&graph, ListInForce::Omit(&omit), &index),
        vec![
            ("urn:ngsi-ld:T:P", ListInForce::Omit(&omit)),
            ("urn:ngsi-ld:T:C", ListInForce::Omit(&omit)),
            ("urn:ngsi-ld:T:K", ListInForce::Omit(&omit)),
        ]
    );
}

#[test]
fn no_projection_reaches_every_traversed_entity() {
    let (graph, index) = (building(), keys());
    let reached: Vec<&str> = written(&graph, ListInForce::Unrestricted, &index).into_iter().map(|(id, _)| id).collect();
    assert_eq!(reached, vec!["urn:ngsi-ld:T:P", "urn:ngsi-ld:T:C", "urn:ngsi-ld:T:K"]);
}

/// Every root is walked before any level-one entity, so `Q` is first
/// encountered through `Z`'s `linked` rather than through `P`'s `x`.
#[test]
fn encounter_order_is_breadth_first() {
    let graph = graph(
        &[node("A", &[("owner", "P")]), node("Z", &[("linked", "Q")])],
        vec![node("P", &[("x", "Q")]), node("Q", &[]).with_properties(&["name", "size"])],
        2,
    );
    let index = keys();
    let pick = [term("owner", vec![term("x", vec![leaf("name")])]), term("linked", vec![leaf("size")])];
    let size = [leaf("size")];
    let reached = written(&graph, ListInForce::Pick(&pick), &index);
    assert_eq!(
        reached.iter().find(|(id, _)| *id == "urn:ngsi-ld:T:Q").map(|(_, list)| *list),
        Some(ListInForce::Pick(&size))
    );
}

#[test]
fn nothing_is_reached_deeper_than_the_traversal() {
    let mut graph = building();
    graph.depth = NonZeroU32::MIN;
    let index = keys();
    let reached: Vec<Identifier> = FlatEncounters::walk::<Nodes>(&graph, MemberRule::new(ListInForce::Unrestricted, &index))
        .written()
        .map(|(id, _)| id.clone())
        .collect();
    assert_eq!(reached.len(), 2, "only P and C sit one hop away: {reached:?}");
}
