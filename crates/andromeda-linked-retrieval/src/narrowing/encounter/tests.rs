//! The first-encounter walk over the Building graph: `A owner→P`,
//! `A manager→P`, `A locatedIn→C`, `P employer→K`, two levels deep.

use crate::{
    graph::LinkedGraph,
    narrowing::{encounter::FlatEncounters, list::ListInForce, list_terms::ListTerms, member::MemberRule},
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
        written(&graph, ListInForce::Addressed(ListTerms::Pick(&pick)), &index),
        vec![("urn:ngsi-ld:T:P", ListInForce::Addressed(ListTerms::Pick(&name)))]
    );
}

/// The pick list, not the stored member order, decides which Relationship
/// reaching a shared target is its first encounter.
#[test]
fn the_pick_list_orders_the_encounters_of_one_entity() {
    let (graph, index) = (building(), keys());
    let pick = [term("manager", vec![leaf("email")]), term("owner", vec![leaf("name")])];
    let email = [leaf("email")];
    assert_eq!(
        written(&graph, ListInForce::Addressed(ListTerms::Pick(&pick)), &index),
        vec![("urn:ngsi-ld:T:P", ListInForce::Addressed(ListTerms::Pick(&email)))]
    );
    let bare_first = [leaf("manager"), term("owner", vec![leaf("name")])];
    assert_eq!(
        written(&graph, ListInForce::Addressed(ListTerms::Pick(&bare_first)), &index),
        vec![("urn:ngsi-ld:T:P", ListInForce::Unrestricted), ("urn:ngsi-ld:T:K", ListInForce::Unrestricted)]
    );
}

/// Clause 5.7.1.5: only the returned Attributes bring their targets.
#[test]
fn a_relationship_the_list_removes_reaches_nothing() {
    let (graph, index) = (building(), keys());
    let pick = [ProjectionTerm::Id, term("owner", vec![leaf("name")])];
    let reached: Vec<&str> = written(&graph, ListInForce::Addressed(ListTerms::Pick(&pick)), &index)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(reached, vec!["urn:ngsi-ld:T:P"]);
}

/// Clause 4.21: a group addresses the entity its Relationship reaches, so
/// the entity reached from there through a Relationship no group names is
/// whole.
#[test]
fn a_group_reaches_only_the_entity_its_relationship_links_to() {
    let (graph, index) = (building(), keys());
    let pick = [ProjectionTerm::Id, term("owner", vec![leaf("name"), leaf("employer")])];
    let group = [leaf("name"), leaf("employer")];
    assert_eq!(
        written(&graph, ListInForce::Addressed(ListTerms::Pick(&pick)), &index),
        vec![
            ("urn:ngsi-ld:T:P", ListInForce::Addressed(ListTerms::Pick(&group))),
            ("urn:ngsi-ld:T:K", ListInForce::Unrestricted)
        ]
    );
}

/// Clause 4.21: with a Linked Entity Term in the list, its plain terms
/// address the roots alone, and every entity no group addresses is whole.
#[test]
fn a_nested_omit_leaves_every_unaddressed_entity_whole() {
    let (graph, index) = (building(), keys());
    let omit = [ProjectionTerm::Type, term("owner", vec![leaf("name")])];
    let name = [leaf("name")];
    assert_eq!(
        written(&graph, ListInForce::Addressed(ListTerms::Omit(&omit)), &index),
        vec![
            ("urn:ngsi-ld:T:P", ListInForce::Addressed(ListTerms::Omit(&name))),
            ("urn:ngsi-ld:T:C", ListInForce::Unrestricted),
            ("urn:ngsi-ld:T:K", ListInForce::Unrestricted),
        ]
    );
}

/// Clause 5.7.1.5: a plain list "applies to all Entities in the payload".
#[test]
fn a_bare_term_passes_its_list_down() {
    let (graph, index) = (building(), keys());
    let pick = [ProjectionTerm::Id, leaf("name"), leaf("owner")];
    assert_eq!(
        written(&graph, ListInForce::Payload(ListTerms::Pick(&pick)), &index),
        vec![("urn:ngsi-ld:T:P", ListInForce::Payload(ListTerms::Pick(&pick)))]
    );
}

#[test]
fn a_plain_omit_reaches_every_entity() {
    let (graph, index) = (building(), keys());
    let omit = [leaf("email")];
    assert_eq!(
        written(&graph, ListInForce::Payload(ListTerms::Omit(&omit)), &index),
        vec![
            ("urn:ngsi-ld:T:P", ListInForce::Payload(ListTerms::Omit(&omit))),
            ("urn:ngsi-ld:T:C", ListInForce::Payload(ListTerms::Omit(&omit))),
            ("urn:ngsi-ld:T:K", ListInForce::Payload(ListTerms::Omit(&omit))),
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
    let reached = written(&graph, ListInForce::Addressed(ListTerms::Pick(&pick)), &index);
    assert_eq!(
        reached.iter().find(|(id, _)| *id == "urn:ngsi-ld:T:Q").map(|(_, list)| *list),
        Some(ListInForce::Addressed(ListTerms::Pick(&size)))
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
