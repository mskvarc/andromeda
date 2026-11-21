//! The terms of a filter drawn from one Entity of its case, the anchor, over the Attributes it holds after its
//! creation or after its change.
//!
//! A term over an Attribute the anchor holds reads as the Attribute's value satisfies it, and is drawn with the
//! opposite polarity now and then, so that the anchored terms of a filter fail for the anchor too. A term over a
//! Relationship naming an Entity of the case may read through it instead, one hop, an Attribute that Linked Entity
//! holds in the same state as the anchor. The Linked Entity is at a lower position and the writes of each round go in
//! position order, so it is in that state whenever the anchor is read. Its Linked Entity path may carry a type hint
//! naming the type of the case, which every drawn Entity has, or another type, which no drawn Entity has. A term,
//! over the anchor's own Attribute or the Linked Entity's, has the filter's `jsonKeys` or `expandValues` name its
//! Attribute when it reads a `JsonProperty` member or a `VocabProperty` value.

use crate::q_generation::{
    drawn_attribute::DrawnAttribute,
    drawn_entity::{DrawnEntity, DrawnState},
    drawn_pool::OTHER_TYPE,
    drawn_query::DrawnQuery,
    free_term::free_term,
    held_term::{Comparison, HeldTerm, held_term},
    placeholder::ENTITY_TYPE,
};
use andromeda_q_corpus::corpus::Traversal;
use proptest::{
    prelude::{BoxedStrategy, Just, Strategy, prop_oneof},
    strategy::Union,
};

/// The weight of a term over an Attribute the anchor holds.
const OWN: u32 = 2;

/// The weight of a term through a Relationship the anchor holds to an Entity of the case.
const HOP: u32 = 2;

/// Whether a drawn term keeps the polarity it is drawn with.
#[derive(Debug, Clone, Copy)]
enum Polarity {
    /// It keeps it and holds for the value it is drawn from.
    Kept,
    /// It takes the opposite one and fails for that value.
    Flipped,
}

/// `term`, flipped to the opposite polarity one time in ten. A test of existence keeps its polarity.
fn polarised(term: BoxedStrategy<HeldTerm>) -> BoxedStrategy<HeldTerm> {
    let polarity = prop_oneof![9 => Just(Polarity::Kept), 1 => Just(Polarity::Flipped)];
    (term, polarity)
        .prop_map(|(term, polarity)| match polarity {
            Polarity::Kept => term,
            Polarity::Flipped => HeldTerm {
                comparison: term.comparison.map(Comparison::flipped),
                ..term
            },
        })
        .boxed()
}

/// A term over `attribute` of the anchor, with the request members it needs.
fn own_term(attribute: &DrawnAttribute) -> BoxedStrategy<DrawnQuery> {
    let name = attribute.name;
    polarised(held_term(attribute))
        .prop_map(move |term| {
            let (json_keys, expand_values) = term.need.listed(name);
            DrawnQuery {
                text: format!("{}{}", term.path_over(name), term.comparison_text()),
                json_keys,
                expand_values,
                traversal: Traversal::Local,
            }
        })
        .boxed()
}

/// The type hint of a Linked Entity path: none most often, the type of the case, or a type no drawn Entity has.
fn type_hint() -> BoxedStrategy<String> {
    prop_oneof![
        4 => Just(String::new()),
        2 => Just(format!("{ENTITY_TYPE}:")),
        1 => Just(format!("{OTHER_TYPE}:")),
    ]
    .boxed()
}

/// A term through the Relationship `relationship` over the Attribute `linked` of its Linked Entity, with the request
/// members it needs.
fn linked_term(relationship: &'static str, linked: &DrawnAttribute) -> BoxedStrategy<DrawnQuery> {
    let name = linked.name;
    (polarised(held_term(linked)), type_hint())
        .prop_map(move |(term, hint)| {
            let (json_keys, expand_values) = term.need.listed(name);
            DrawnQuery {
                text: format!("{relationship}{{{hint}{}}}{}", term.path_over(name), term.comparison_text()),
                json_keys,
                expand_values,
                traversal: Traversal::OneHop,
            }
        })
        .boxed()
}

/// A term through `attribute` of the anchor, in `state`, to an Attribute of an Entity of `entities` it names, None
/// when it is no Relationship or names no Entity of the case.
fn hop_term(entities: &[DrawnEntity], attribute: &DrawnAttribute, state: DrawnState) -> Option<BoxedStrategy<DrawnQuery>> {
    let options: Vec<(u32, BoxedStrategy<DrawnQuery>)> = attribute
        .family
        .objects()?
        .iter()
        .filter_map(|object| object.position())
        .filter_map(|target| entities.get(target))
        .flat_map(|linked| linked.held(state))
        .map(|linked| (1, linked_term(attribute.name, linked)))
        .collect();
    (!options.is_empty()).then(|| Union::new_weighted(options).boxed())
}

/// A term drawn from the Entity at `position` of `entities` in `state`: over one of its Attributes, or through one of
/// its Relationships to an Entity of the case.
pub fn anchored_term(entities: &[DrawnEntity], position: usize, state: DrawnState) -> BoxedStrategy<DrawnQuery> {
    let Some(anchor) = entities.get(position) else {
        return free_term();
    };
    let mut options = Vec::new();
    for attribute in anchor.held(state) {
        options.push((OWN, own_term(attribute)));
        if let Some(hop) = hop_term(entities, attribute, state) {
            options.push((HOP, hop));
        }
    }
    if options.is_empty() {
        return free_term();
    }
    Union::new_weighted(options).boxed()
}
