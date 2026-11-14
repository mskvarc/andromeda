use crate::q_reading::{BaselineComparison, compare_with_baseline, is_scanned, source_sites};
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;

/// A production source outside the owner.
const OUTSIDE: &str = "crates/andromeda-subscription/src/matching.rs";

fn sites(path: &str, source: &str) -> Vec<String> {
    let syntax = syn::parse_file(source).unwrap_or_else(|error| panic!("fixture parses: {error}"));
    source_sites(path, &syntax)
}

#[test]
fn a_match_on_a_predicate_variant_outside_the_owner_is_reported_with_its_path_and_line() {
    let source = "fn f(pred: &Predicate) -> bool {\n    match pred {\n        Predicate::Equality { .. } => true,\n        other => false,\n    }\n}\n";
    assert_eq!(
        sites(OUTSIDE, source),
        vec!["crates/andromeda-subscription/src/matching.rs:3: matches on `Predicate::Equality`".to_owned()]
    );
}

#[test]
fn an_if_let_on_a_predicate_variant_outside_the_owner_is_reported() {
    let source = "fn f(pred: &Predicate) { if let Predicate::Exists(path) = pred { use_it(path); } }";
    assert_eq!(sites(OUTSIDE, source).len(), 1);
}

#[test]
fn a_call_to_an_accessor_is_not_reported() {
    assert_eq!(sites(OUTSIDE, "fn f(pred: &Predicate) -> &QueryPath { pred.path() }"), Vec::<String>::new());
}

#[test]
fn building_a_variant_is_not_reported() {
    assert_eq!(
        sites(OUTSIDE, "fn f(path: QueryPath) -> Predicate { Predicate::Exists(path) }"),
        Vec::<String>::new()
    );
}

#[test]
fn the_same_patterns_inside_a_test_module_are_not_reported() {
    let source = "#[cfg(test)]\nmod tests {\n    fn f(pred: &Predicate) -> bool {\n        if let Predicate::Exists(_) = pred { return true; }\n        matches!(pred, Predicate::Exists(_))\n    }\n}\n";
    assert_eq!(sites(OUTSIDE, source), Vec::<String>::new());
}

#[test]
fn a_predicate_variant_inside_matches_outside_the_owner_is_reported() {
    let source = "fn f(pred: &Predicate) -> bool { matches!(pred, Predicate::Exists(_)) }";
    assert_eq!(sites(OUTSIDE, source).len(), 1);
}

#[test]
fn a_matches_nested_in_another_macro_is_reported() {
    let source = "fn f(pred: &Predicate) { debug_assert!(matches!(pred, CompareValue::Range { .. } | CompareValue::ValueList(_))); }";
    assert_eq!(sites(OUTSIDE, source).len(), 2);
}

#[test]
fn the_expression_a_matches_tests_is_not_its_pattern() {
    let source = "fn f(value: &Value) -> bool { matches!(convert(CompareValue::Scalar(x)), Value::Number(_)) }";
    assert_eq!(sites(OUTSIDE, source), Vec::<String>::new());
}

#[test]
fn every_term_type_is_read() {
    for owner in ["Predicate", "SetMembership", "OrderingOp", "CompareValue"] {
        let source = format!("fn f(x: &X) -> bool {{ match x {{ {owner}::A => true, other => false }} }}");
        assert_eq!(sites(OUTSIDE, &source).len(), 1, "{owner}");
    }
}

#[test]
fn the_owner_directory_and_test_files_are_not_scanned() {
    assert!(!is_scanned("crates/andromeda-ngsi-ld-query/src/filter/eval.rs"));
    assert!(!is_scanned("crates/andromeda-subscription/src/matching_tests.rs"));
    assert!(is_scanned("crates/andromeda-ngsi-ld-query/src/split_filter.rs"));
    assert!(is_scanned(OUTSIDE));
}

fn found(entries: &[(&str, usize)]) -> BTreeMap<String, Vec<String>> {
    entries
        .iter()
        .map(|(path, count)| {
            (
                (*path).to_owned(),
                (1..=*count).map(|line| format!("{path}:{line}: matches on `Predicate::Exists`")).collect(),
            )
        })
        .collect()
}

#[test]
fn a_file_within_its_baseline_count_passes() {
    assert_eq!(compare_with_baseline(&found(&[("a.rs", 2)]), &[("a.rs", 2)]), BaselineComparison::default());
}

#[test]
fn a_file_past_its_baseline_count_reports_its_sites() {
    let comparison = compare_with_baseline(&found(&[("a.rs", 3), ("b.rs", 1)]), &[("a.rs", 2)]);
    assert_eq!(comparison.unexpected.len(), 4);
    assert_eq!(comparison.stale, Vec::<String>::new());
}

#[test]
fn a_file_below_its_baseline_count_is_stale() {
    let comparison = compare_with_baseline(&found(&[("a.rs", 1)]), &[("a.rs", 2), ("c.rs", 1)]);
    assert_eq!(comparison.unexpected, Vec::<String>::new());
    assert_eq!(
        comparison.stale,
        vec!["a.rs: 1 sites, baseline 2".to_owned(), "c.rs: 0 sites, baseline 1".to_owned()]
    );
}
