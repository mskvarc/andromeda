//! A subscription's entity selection reduced to what one registration offers
//! (clauses 4.3.6.1, 5.8.1.4 and 5.12).
//!
//! Clause 5.8.1.4 places at a context source "a copy of the original
//! Subscription ... reduced to what is matched by the registration
//! information", and clause 4.3.6.1 holds every distributed request to the
//! entity identifiers and types a registration declares, since the source
//! cannot tell which registration a request came through. Each selector is
//! therefore intersected with each registered `EntityInfo`: its type selector
//! is cut to the alternatives the registered types satisfy, and its identifier
//! constraint becomes the narrower of the two. A selector yields a reduced
//! selector for an `EntityInfo` exactly when it matches that `EntityInfo`
//! under clause 5.12, so the reduction never disagrees with the matching that
//! decides whether a copy is placed.
//!
//! Two identifier patterns cannot be intersected into one IEEE 1003.2 pattern,
//! the reason clause 5.12 takes them as matching. The reduced selector then
//! carries the registered pattern, the constraint the source is held to, and
//! the subscription's own pattern is applied by the broker that relays the
//! notifications.

use crate::{
    entity_info::{EntityInfo, EntityTypeSelection},
    registration::info::RegistrationInfo,
    subscription::entity_selector::EntitySelector,
};
use andromeda_ngsi_ld_query::{identifier_axis::IdentifierAxis, type_selection::grammar::TypeExprRaw};
use std::slice;

/// The entity selectors of a subscription reduced to what the `information`
/// of one registration offers.
///
/// `None` when the reduction constrains no entity: the subscription names no
/// selector and some block registers no entity. Otherwise the reduced
/// selectors in the subscription's order, each written once, and empty when no
/// selector matches any block. A subscription naming no selector selects every
/// entity, so it reduces to the entities the blocks register.
#[must_use]
pub fn reduced_selectors(selectors: Option<&[EntitySelector]>, information: &[RegistrationInfo]) -> Option<Vec<EntitySelector>> {
    let every;
    let selectors = match selectors {
        Some(selectors) if !selectors.is_empty() => selectors,
        Some(_) | None => {
            if information.iter().any(|block| block.entities.is_none()) {
                return None;
            }
            every = EntitySelector {
                id: None,
                id_pattern: None,
                type_: TypeExprRaw::Wildcard,
            };
            slice::from_ref(&every)
        }
    };
    let mut reduced = Vec::new();
    for selector in selectors {
        for block in information {
            match block.entities.as_deref() {
                // Clause 5.12: a block registering no entity matches every
                // selector and constrains none.
                None => push_once(&mut reduced, selector.clone()),
                Some(entities) => entities
                    .iter()
                    .filter_map(|info| narrowed_selector(selector, info))
                    .for_each(|narrowed| push_once(&mut reduced, narrowed)),
            }
        }
    }
    Some(reduced)
}

/// `selector` narrowed to the entities `info` registers, or `None` when it
/// matches none of them (clause 5.12).
fn narrowed_selector(selector: &EntitySelector, info: &EntityInfo) -> Option<EntitySelector> {
    let type_ = narrowed_type(&selector.type_, &info.type_)?;
    let (id, id_pattern) = match (selector.identifier_axis(), IdentifierAxis::of(info.id.as_slice(), info.id_pattern.as_ref())) {
        // A registration constraining no identifier leaves the selector's own
        // constraint, and a selector identifier the registration admits is
        // already the narrower one.
        (IdentifierAxis::Any | IdentifierAxis::Pattern(_), IdentifierAxis::Any) => (selector.id.clone(), selector.id_pattern.clone()),
        (IdentifierAxis::Ids(ids), registered) => {
            if !ids.iter().any(|id| registered.admits(id)) {
                return None;
            }
            (selector.id.clone(), selector.id_pattern.clone())
        }
        // The registered identifier is narrower than a selector constraining
        // none, or than a selector pattern matching it.
        (IdentifierAxis::Any, IdentifierAxis::Ids(registered)) => (registered.first().cloned(), None),
        (IdentifierAxis::Pattern(pattern), IdentifierAxis::Ids(registered)) => {
            (Some(registered.iter().find(|id| pattern.is_match(id.as_str()))?.clone()), None)
        }
        (IdentifierAxis::Any | IdentifierAxis::Pattern(_), IdentifierAxis::Pattern(registered)) => (None, Some(registered.clone())),
    };
    Some(EntitySelector { id, id_pattern, type_ })
}

/// The alternatives of `selected` the `registered` types satisfy, as one
/// clause 4.17 selector, or `None` when none does.
///
/// An alternative is kept whole when the registered types satisfy it, the
/// reading clause 5.12 gives a type selector against an `EntityInfo`. The
/// wildcard becomes the registered types themselves, so no type outside the
/// registration reaches the source (clause 4.3.6.1).
fn narrowed_type(selected: &TypeExprRaw, registered: &EntityTypeSelection) -> Option<TypeExprRaw> {
    let mut kept = Vec::new();
    collect_covered(selected, registered, &mut kept);
    TypeExprRaw::any_of(kept)
}

/// Push onto `kept` each alternative of `selected` the `registered` types
/// satisfy, the wildcard standing for every registered type.
fn collect_covered(selected: &TypeExprRaw, registered: &EntityTypeSelection, kept: &mut Vec<TypeExprRaw>) {
    match selected {
        TypeExprRaw::Or(alternatives) => alternatives.iter().for_each(|alternative| collect_covered(alternative, registered, kept)),
        TypeExprRaw::Wildcard => registered.names().iter().for_each(|name| push_once(kept, TypeExprRaw::Type(name.clone()))),
        TypeExprRaw::Type(_) | TypeExprRaw::And(_) => {
            if selected.matches(registered.names()) {
                push_once(kept, selected.clone());
            }
        }
    }
}

/// Append `item` unless `items` already holds an equal one.
fn push_once<T: PartialEq>(items: &mut Vec<T>, item: T) {
    if !items.contains(&item) {
        items.push(item);
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        registration::info::RegistrationInfo,
        registration_matching::{
            entity_specification::EntitySpecification,
            selector_reduction::{narrowed_selector, reduced_selectors},
        },
        subscription::entity_selector::EntitySelector,
    };
    use serde_json::{Value, json};

    const VEHICLE: &str = "https://example.org/Vehicle";
    const BUS: &str = "https://example.org/Bus";
    const ELECTRIC: &str = "https://example.org/Electric";
    const SEVEN: &str = "urn:ngsi-ld:Vehicle:7";

    fn selector(wire: Value) -> EntitySelector {
        serde_json::from_value(wire).expect("a readable selector")
    }

    fn selectors(wire: Value) -> Vec<EntitySelector> {
        serde_json::from_value(wire).expect("readable selectors")
    }

    fn information(wire: Value) -> Vec<RegistrationInfo> {
        serde_json::from_value(wire).expect("readable information")
    }

    /// The selectors `subscribed` reduce to against `registered`, as written.
    fn reduced(subscribed: &Value, registered: Value) -> Option<Value> {
        let subscribed = (!subscribed.is_null()).then(|| selectors(subscribed.clone()));
        reduced_selectors(subscribed.as_deref(), &information(registered)).map(|reduced| serde_json::to_value(reduced).expect("selectors write"))
    }

    /// Clause 4.3.6.1: a registration naming one entity is offered for that
    /// entity alone, so a type-only selector reaches the source naming it.
    #[test]
    fn a_type_only_selector_takes_the_registered_id() {
        pretty_assertions::assert_eq!(
            reduced(&json!([{"type": VEHICLE}]), json!([{"entities": [{"type": VEHICLE, "id": SEVEN}]}])),
            Some(json!([{"type": VEHICLE, "id": SEVEN}]))
        );
    }

    #[test]
    fn a_type_only_selector_takes_the_registered_pattern() {
        pretty_assertions::assert_eq!(
            reduced(
                &json!([{"type": VEHICLE}]),
                json!([{"entities": [{"type": VEHICLE, "idPattern": "^urn:ngsi-ld:Vehicle:A"}]}])
            ),
            Some(json!([{"type": VEHICLE, "idPattern": "^urn:ngsi-ld:Vehicle:A"}]))
        );
    }

    /// Clause 5.12: a selector pattern matching the registered `id` matches,
    /// and the registered `id` is the narrower constraint; a pattern the `id`
    /// does not match leaves nothing.
    #[test]
    fn a_selector_pattern_takes_the_registered_id_it_matches() {
        let registered = json!([{"entities": [{"type": VEHICLE, "id": SEVEN}]}]);
        pretty_assertions::assert_eq!(
            reduced(&json!([{"type": VEHICLE, "idPattern": "^urn:ngsi-ld:Vehicle:"}]), registered.clone()),
            Some(json!([{"type": VEHICLE, "id": SEVEN}]))
        );
        pretty_assertions::assert_eq!(
            reduced(&json!([{"type": VEHICLE, "idPattern": "^urn:ngsi-ld:Bus:"}]), registered),
            Some(json!([]))
        );
    }

    /// Clause 5.2.8: the registered `id` decides over the pattern beside it.
    #[test]
    fn a_registered_id_beside_a_pattern_is_the_registered_constraint() {
        let registered = json!([{"entities": [{"type": VEHICLE, "id": SEVEN, "idPattern": ".*"}]}]);
        pretty_assertions::assert_eq!(
            reduced(&json!([{"type": VEHICLE}]), registered.clone()),
            Some(json!([{"type": VEHICLE, "id": SEVEN}]))
        );
        pretty_assertions::assert_eq!(reduced(&json!([{"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:1"}]), registered), Some(json!([])));
    }

    /// A selector identifier the registration admits is already narrower, so
    /// the selector reaches the source as written.
    #[test]
    fn a_selector_id_the_registration_admits_is_kept_as_written() {
        let written = json!({"type": VEHICLE, "id": SEVEN, "idPattern": "^urn:ngsi-ld:Bus:"});
        pretty_assertions::assert_eq!(
            reduced(
                &json!([written.clone()]),
                json!([{"entities": [{"type": VEHICLE, "idPattern": "^urn:ngsi-ld:Vehicle:"}]}])
            ),
            Some(json!([written]))
        );
    }

    /// Clause 5.12 takes two patterns as matching; the registered one is the
    /// constraint the source is held to (clause 4.3.6.1).
    #[test]
    fn overlapping_patterns_reach_the_source_as_the_registered_pattern() {
        pretty_assertions::assert_eq!(
            reduced(
                &json!([{"type": VEHICLE, "idPattern": "^urn:ngsi-ld:Vehicle:A"}]),
                json!([{"entities": [{"type": VEHICLE, "idPattern": "7$"}]}])
            ),
            Some(json!([{"type": VEHICLE, "idPattern": "7$"}]))
        );
    }

    /// Clause 4.3.6.1: "the distributed request does not contain additional
    /// types", so an alternation keeps only the alternatives the registered
    /// types satisfy, a conjunction staying whole when they satisfy all of it.
    #[test]
    fn a_type_selector_keeps_only_the_registered_alternatives() {
        let vehicle_or_bus = json!([{"type": format!("{VEHICLE}|{BUS}")}]);
        pretty_assertions::assert_eq!(
            reduced(&vehicle_or_bus, json!([{"entities": [{"type": VEHICLE}]}])),
            Some(json!([{"type": VEHICLE}]))
        );
        pretty_assertions::assert_eq!(
            reduced(&vehicle_or_bus, json!([{"entities": [{"type": [VEHICLE, BUS, ELECTRIC]}]}])),
            Some(json!([{"type": format!("{VEHICLE},{BUS}")}]))
        );
        let conjunction = json!([{"type": format!("({VEHICLE};{ELECTRIC})|{BUS}")}]);
        pretty_assertions::assert_eq!(
            reduced(&conjunction, json!([{"entities": [{"type": [VEHICLE, ELECTRIC]}]}])),
            Some(json!([{"type": format!("({VEHICLE};{ELECTRIC})")}]))
        );
        pretty_assertions::assert_eq!(reduced(&conjunction, json!([{"entities": [{"type": VEHICLE}]}])), Some(json!([])));
    }

    /// Each selector is intersected with every registered entry it matches,
    /// in the subscription's order, and an intersection two entries share is
    /// written once.
    #[test]
    fn every_entry_a_selector_matches_contributes_and_duplicates_are_written_once() {
        pretty_assertions::assert_eq!(
            reduced(
                &json!([{"type": BUS}, {"type": VEHICLE}]),
                json!([
                    {"entities": [{"type": VEHICLE, "id": SEVEN}, {"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:8"}]},
                    {"entities": [{"type": [VEHICLE, BUS]}]},
                    {"entities": [{"type": VEHICLE, "id": SEVEN}], "propertyNames": ["https://example.org/speed"]}
                ])
            ),
            Some(json!([
                {"type": BUS},
                {"type": VEHICLE, "id": SEVEN},
                {"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:8"},
                {"type": VEHICLE}
            ]))
        );
    }

    /// Clause 5.12: a block registering no entity matches every selector and
    /// constrains none of them.
    #[test]
    fn a_block_registering_no_entity_keeps_the_selectors_whole() {
        pretty_assertions::assert_eq!(
            reduced(
                &json!([{"type": VEHICLE, "idPattern": "^urn:"}]),
                json!([{"propertyNames": ["https://example.org/speed"]}])
            ),
            Some(json!([{"type": VEHICLE, "idPattern": "^urn:"}]))
        );
    }

    /// A subscription naming no selector selects every entity, so it reduces
    /// to the entities each block registers, and to no constraint at all when
    /// some block registers no entity.
    #[test]
    fn a_subscription_without_selectors_takes_the_registered_entities() {
        pretty_assertions::assert_eq!(
            reduced(
                &Value::Null,
                json!([{"entities": [{"type": [VEHICLE, BUS], "id": SEVEN}, {"type": BUS, "idPattern": "^urn:ngsi-ld:Bus:"}]}])
            ),
            Some(json!([{"type": format!("{VEHICLE},{BUS}"), "id": SEVEN}, {"type": BUS, "idPattern": "^urn:ngsi-ld:Bus:"}]))
        );
        pretty_assertions::assert_eq!(
            reduced(
                &Value::Null,
                json!([{"entities": [{"type": VEHICLE}]}, {"propertyNames": ["https://example.org/speed"]}])
            ),
            None
        );
    }

    /// The reduction yields a selector for an entry exactly when clause 5.12
    /// matches the selector against it, so it never decides a copy differently
    /// from the matching that placed it.
    #[test]
    fn a_selector_reduces_against_an_entry_exactly_when_it_matches_it() {
        let selectors = selectors(json!([
            {"type": VEHICLE},
            {"type": BUS},
            {"type": format!("{VEHICLE}|{BUS}")},
            {"type": format!("({VEHICLE};{ELECTRIC})")},
            {"type": VEHICLE, "id": SEVEN},
            {"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:1"},
            {"type": VEHICLE, "idPattern": "^urn:ngsi-ld:Vehicle:"},
            {"type": VEHICLE, "idPattern": "^urn:ngsi-ld:Bus:"},
            {"type": VEHICLE, "id": SEVEN, "idPattern": "^urn:ngsi-ld:Bus:"}
        ]));
        let entries = json!([
            {"type": VEHICLE},
            {"type": [BUS, ELECTRIC]},
            {"type": [VEHICLE, ELECTRIC]},
            {"type": VEHICLE, "id": SEVEN},
            {"type": VEHICLE, "idPattern": "^urn:ngsi-ld:Vehicle:"},
            {"type": VEHICLE, "id": SEVEN, "idPattern": "^urn:ngsi-ld:Bus:"}
        ]);
        for entry in entries.as_array().expect("an array of entries") {
            let block = information(json!([{"entities": [entry]}])).remove(0);
            let info = block.entities.as_deref().and_then(<[_]>::first).expect("one entry");
            for selector in &selectors {
                pretty_assertions::assert_eq!(
                    narrowed_selector(selector, info).is_some(),
                    EntitySpecification::from_selector(selector).matches_entity_axis(&block),
                    "{selector:?} against {entry}"
                );
            }
        }
    }

    #[test]
    fn a_wildcard_selector_takes_the_registered_types() {
        let every = selector(json!({"type": "*"}));
        let block = information(json!([{"entities": [{"type": [VEHICLE, BUS]}]}])).remove(0);
        let info = block.entities.as_deref().and_then(<[_]>::first).expect("one entry");
        pretty_assertions::assert_eq!(
            serde_json::to_value(narrowed_selector(&every, info)).expect("writes"),
            json!({"type": format!("{VEHICLE},{BUS}")})
        );
    }
}
