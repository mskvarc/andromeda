//! A prepared filter, with its terms numbered or not, decides every state as
//! the reading it was prepared from does, and its required attributes never
//! refuse a state the filter holds for.
//!
//! The states are those a Subscription's filter reads around a write: the
//! Entity as a creation leaves it, as a partial update that leaves an
//! attribute untouched leaves it, as the deletion of an attribute leaves it,
//! and the Entity a deletion removed, which has nothing left to read.

use crate::{
    q::{
        filter_run::filter_holds,
        normal_form_tests::{CORPUS_ENTITIES, corpus_path, decoded, filter_over, generated_case, generated_iri},
        prepared_filter::PreparedFilter,
        term_memo::{TermInterner, TermMemo},
    },
    queried_entity::expanded_fixture::{FixtureEntity, FixtureLinked},
};
use andromeda_ngsi_ld_query::filter::{Filter, language_matching::LanguageMatching, reading::filter_reading::FilterReading};
use proptest::{
    prelude::{prop_assert_eq, proptest},
    test_runner::Config,
};
use serde_json::{Value, json};

/// The members of an expanded Entity document that are not attributes.
const IDENTITY: [&str; 2] = ["@id", "@type"];

/// The expanded name of the Property a partial update writes.
const UPDATED: &str = "a";

/// The Entity `document` holds as a creation leaves it, as an update of
/// `a` that touches no other attribute leaves it, as the deletion of each of
/// its attributes leaves it, and with no attribute at all.
fn states(document: &Value) -> Vec<Value> {
    let Some(members) = document[0].as_object() else {
        return vec![document.clone()];
    };
    let attributes = members.keys().filter(|key| !IDENTITY.contains(&key.as_str()));
    let mut states = vec![document.clone()];
    let mut updated = document.clone();
    updated[0][generated_iri(UPDATED)] = json!([{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 10}]}]);
    states.push(updated);
    states.extend(attributes.map(|key| {
        let mut deleted = document.clone();
        if let Some(members) = deleted[0].as_object_mut() {
            members.remove(key);
        }
        deleted
    }));
    let mut removed = document.clone();
    if let Some(members) = removed[0].as_object_mut() {
        members.retain(|key, _| IDENTITY.contains(&key.as_str()));
    }
    states.push(removed);
    states
}

/// The first state of `states` over which `prepared` and `reading` disagree,
/// or over which the required attributes refuse what `reading` holds for.
fn disagreement(text: &str, states: &[FixtureEntity], linked: &[FixtureEntity]) -> Option<String> {
    let expr = text
        .parse::<Filter>()
        .unwrap_or_else(|error| panic!("the generator writes grammatical filters, and {text} is refused: {error}"));
    [LanguageMatching::TagEquality, LanguageMatching::ExtendedFiltering]
        .into_iter()
        .find_map(|matching| {
            let reading = FilterReading::of(&expr, matching);
            let prepared = PreparedFilter::new(&reading);
            let interner = TermInterner::new();
            let mut numbered = prepared.clone();
            numbered.number_terms(&interner);
            let mut memo = TermMemo::new(&interner);
            states.iter().enumerate().find_map(|(index, state)| {
                let held = filter_holds(&reading, state, FixtureLinked(linked));
                let ran = prepared.holds(state, FixtureLinked(linked));
                if held != ran {
                    return Some(format!("state {index} under {matching:?}: the reading says {held}, the prepared filter {ran}"));
                }
                memo.begin_pass();
                let memoised = numbered.holds_memoised(state, FixtureLinked(linked), &mut memo);
                if held != memoised {
                    return Some(format!(
                        "state {index} under {matching:?}: the reading says {held}, the memoised filter {memoised}"
                    ));
                }
                (held && !prepared.required().held_by(state)).then(|| format!("state {index} under {matching:?} holds and lacks a required attribute"))
            })
        })
}

proptest! {
    #![proptest_config(Config::with_cases(2048))]

    /// Over generated Entities, the required attributes never refuse a
    /// state the filter holds for, and the preparation never changes an answer.
    #[test]
    fn the_required_attributes_never_refuse_a_state_a_generated_filter_holds_for((documents, text) in generated_case()) {
        let linked = decoded(&documents);
        let states: Vec<FixtureEntity> = states(&documents[0]).iter().map(FixtureEntity::from_expanded).collect();
        prop_assert_eq!(disagreement(&text, &states, &linked), None, "{}", text);
    }

    /// Over the corpus Entities, each as a creation leaves it.
    #[test]
    fn the_required_attributes_never_refuse_a_corpus_state_a_filter_holds_for(text in filter_over(corpus_path())) {
        prop_assert_eq!(disagreement(&text, &CORPUS_ENTITIES, &CORPUS_ENTITIES), None, "{}", text);
    }
}
