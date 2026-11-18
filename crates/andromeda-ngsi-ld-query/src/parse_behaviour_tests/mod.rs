//! What every parse entry point of the query languages makes of a fixed set
//! of inputs, one snapshot per entry point.
//!
//! An accepted input is recorded with the syntax tree it parses to and, where
//! the tree is written back as text, that text and what reading it again
//! gives. A refused input is recorded with its error, the path of nested
//! variants that names the error, and the detail text a client is answered
//! with.
//!
//! The inputs are hand-written cases, values generated over the entry point's
//! grammar where it has one, one-character mutations of both, and every
//! string up to a few symbols long over the entry point's own alphabet. The
//! generators and the mutations draw from a deterministic random source, so
//! every run reads the same inputs, and a change in what any parser makes of
//! them is one reviewed snapshot diff.

mod dataset_selector_inputs;
mod entry_point;
mod filter_inputs;
mod generated_inputs;
mod mutation;
mod ordering_inputs;
mod outcome;
mod projection_inputs;
mod scope_inputs;
mod short_strings;
mod temporal_inputs;
mod type_selection_inputs;

use crate::parse_behaviour_tests::{
    entry_point::EntryPoint,
    mutation::{Mutation, MutationKind, mutation},
    outcome::{recorded, variant_path},
    short_strings::short_strings,
};
use andromeda_architecture_checks::assert_named_snapshot;
use pretty_assertions::assert_eq;
use proptest::{
    strategy::{Strategy, ValueTree},
    test_runner::TestRunner,
};
use strum::IntoEnumIterator;

#[test]
fn the_q_parse_behaviour_is_pinned() {
    assert_named_snapshot!("parse-behaviour-q", recorded(EntryPoint::Filter));
}

#[test]
fn the_type_selection_parse_behaviour_is_pinned() {
    assert_named_snapshot!("parse-behaviour-type", recorded(EntryPoint::TypeSelection));
}

#[test]
fn the_scope_query_parse_behaviour_is_pinned() {
    assert_named_snapshot!("parse-behaviour-scope-q", recorded(EntryPoint::ScopeQuery));
}

#[test]
fn the_scope_pattern_parse_behaviour_is_pinned() {
    assert_named_snapshot!("parse-behaviour-scope-pattern", recorded(EntryPoint::ScopePattern));
}

#[test]
fn the_scope_label_parse_behaviour_is_pinned() {
    assert_named_snapshot!("parse-behaviour-scope-label", recorded(EntryPoint::ScopeLabel));
}

#[test]
fn the_projection_parse_behaviour_is_pinned() {
    assert_named_snapshot!("parse-behaviour-pick", recorded(EntryPoint::Projection));
}

#[test]
fn the_ordering_parse_behaviour_is_pinned() {
    assert_named_snapshot!("parse-behaviour-order-by", recorded(EntryPoint::Ordering));
}

#[test]
fn the_collation_parse_behaviour_is_pinned() {
    assert_named_snapshot!("parse-behaviour-collation", recorded(EntryPoint::Collation));
}

#[test]
fn the_aggregation_grouping_parse_behaviour_is_pinned() {
    assert_named_snapshot!("parse-behaviour-aggr-group-by", recorded(EntryPoint::Grouping));
}

#[test]
fn the_group_key_parse_behaviour_is_pinned() {
    assert_named_snapshot!("parse-behaviour-group-key", recorded(EntryPoint::GroupKey));
}

#[test]
fn the_value_path_parse_behaviour_is_pinned() {
    assert_named_snapshot!("parse-behaviour-value-path", recorded(EntryPoint::ValuePath));
}

#[test]
fn the_target_language_parse_behaviour_is_pinned() {
    assert_named_snapshot!("parse-behaviour-target-language", recorded(EntryPoint::TargetLanguage));
}

#[test]
fn the_pattern_parse_behaviour_is_pinned() {
    assert_named_snapshot!("parse-behaviour-pattern", recorded(EntryPoint::Pattern));
}

#[test]
fn the_dataset_selector_parse_behaviour_is_pinned() {
    assert_named_snapshot!("parse-behaviour-dataset-id", recorded(EntryPoint::DatasetSelector));
}

#[test]
fn the_aggregation_period_parse_behaviour_is_pinned() {
    assert_named_snapshot!("parse-behaviour-aggr-period-duration", recorded(EntryPoint::AggregationPeriod));
}

#[test]
fn every_corpus_is_the_same_on_every_run() {
    for entry_point in EntryPoint::iter() {
        assert_eq!(entry_point.corpus(), entry_point.corpus(), "{entry_point}");
    }
}

#[test]
fn a_variant_path_follows_the_nested_variants() {
    assert_eq!(
        variant_path("Member(QUnexpectedToken { position: 1, token: Token(LBrace) })"),
        "Member/QUnexpectedToken"
    );
    assert_eq!(
        variant_path("QInvalidDateTime(Lexical(ZoneDesignator))"),
        "QInvalidDateTime/Lexical/ZoneDesignator"
    );
    assert_eq!(variant_path("QEmpty"), "QEmpty");
    assert_eq!(variant_path("TypeExprUnbalanced(2)"), "TypeExprUnbalanced");
}

#[test]
fn a_mutation_changes_one_character_at_its_position() {
    assert_eq!(MutationKind::Delete.applied_at("aéb", 1, 'x'), "ab");
    assert_eq!(MutationKind::Replace.applied_at("aéb", 1, 'x'), "axb");
    assert_eq!(MutationKind::Insert.applied_at("aéb", 1, 'x'), "axéb");
    assert_eq!(MutationKind::Insert.applied_at("aéb", 3, 'x'), "aébx");
}

#[test]
fn every_mutation_of_an_empty_text_inserts() {
    let drawn = mutation().new_tree(&mut TestRunner::deterministic()).expect("a mutation is drawn").current();
    for kind in MutationKind::iter() {
        assert_eq!(Mutation { kind, ..drawn.clone() }.applied_to(""), drawn.character.to_string(), "{kind:?}");
    }
}

#[test]
fn the_short_strings_are_every_string_up_to_the_longest() {
    assert_eq!(short_strings(&["a", "bc"], 2), ["", "a", "bc", "aa", "abc", "bca", "bcbc"]);
}
