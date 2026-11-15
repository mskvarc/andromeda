use crate::q_reading::{is_scanned, source_sites};
use pretty_assertions::assert_eq;

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
    for owner in ["Predicate", "SetMembership", "OrderingOp", "CompareValue", "Scalar"] {
        let source = format!("use crate::filter::scalar::Scalar;\nfn f(x: &X) -> bool {{ match x {{ {owner}::A => true, other => false }} }}");
        assert_eq!(sites(OUTSIDE, &source).len(), 1, "{owner}");
    }
}

#[test]
fn a_scalar_of_another_crate_is_not_a_term_variant() {
    let source = "use crate::expanded_tree::Scalar;\nfn f(x: &Scalar) -> bool { match x { Scalar::Number(_) => true, other => false } }";
    assert_eq!(sites(OUTSIDE, source), Vec::<String>::new());
}

#[test]
fn a_scalar_imported_from_the_query_filter_module_is_a_term_variant() {
    let source = "use andromeda_ngsi_ld_query::filter::scalar::Scalar;\nfn f(x: &Scalar) -> bool { match x { Scalar::Number(_) => true, other => false } }";
    assert_eq!(sites(OUTSIDE, source).len(), 1);
}

#[test]
fn a_scalar_spelled_through_the_query_filter_module_is_a_term_variant() {
    let source = "fn f(x: &X) -> bool { match x { andromeda_ngsi_ld_query::filter::scalar::Scalar::Number(_) => true, other => false } }";
    assert_eq!(sites(OUTSIDE, source).len(), 1);
}

#[test]
fn a_self_variant_in_an_impl_of_a_watched_type_is_reported() {
    let source = "impl OrderingOp {\n    fn f(&self) -> bool { match self { Self::Eq => true, other => false } }\n}\n";
    assert_eq!(
        sites(OUTSIDE, source),
        vec!["crates/andromeda-subscription/src/matching.rs:2: matches on `OrderingOp::Eq`".to_owned()]
    );
}

#[test]
fn a_self_variant_inside_matches_in_an_impl_of_a_watched_type_is_reported() {
    let source = "use crate::filter::scalar::Scalar;\nimpl Scalar { fn f(&self) -> bool { matches!(self, Self::Text(_)) } }";
    assert_eq!(sites(OUTSIDE, source).len(), 1);
}

#[test]
fn a_trait_impl_for_a_watched_type_reads_self_as_that_type() {
    let source = "impl Display for SetMembership { fn fmt(&self) { match self { Self::Match => 1, Self::NoMatch => 2 } } }";
    assert_eq!(sites(OUTSIDE, source).len(), 2);
}

#[test]
fn a_self_variant_in_an_impl_of_another_type_is_not_reported() {
    let source = "impl Other { fn f(&self) -> bool { match self { Self::Eq => true, other => false } } }";
    assert_eq!(sites(OUTSIDE, source), Vec::<String>::new());
}

#[test]
fn self_in_a_nested_impl_of_another_type_does_not_inherit_the_outer_owner() {
    let source = "impl OrderingOp { fn f() { struct Inner; impl Inner { fn g(&self) { match self { Self::Eq => 1, other => 2 }; } } } }";
    assert_eq!(sites(OUTSIDE, source), Vec::<String>::new());
}

#[test]
fn a_variant_imported_by_name_is_reported_where_a_pattern_names_it() {
    let source =
        "use crate::filter::OrderingOp::{Eq, Ne};\nfn f(op: OrderingOp) -> bool {\n    match op {\n        Eq => true,\n        Ne => false,\n    }\n}\n";
    assert_eq!(
        sites(OUTSIDE, source),
        vec![
            "crates/andromeda-subscription/src/matching.rs:4: matches on `OrderingOp::Eq`".to_owned(),
            "crates/andromeda-subscription/src/matching.rs:5: matches on `OrderingOp::Ne`".to_owned(),
        ]
    );
}

#[test]
fn an_imported_variant_with_a_payload_is_reported_in_a_tuple_struct_pattern() {
    let source = "use Predicate::Exists;\nfn f(pred: &Predicate) { if let Exists(path) = pred { use_it(path); } }";
    assert_eq!(sites(OUTSIDE, source).len(), 1);
}

#[test]
fn a_variant_imported_under_a_new_name_is_reported_under_that_name() {
    let source = "use crate::filter::scalar::Scalar;\nuse Scalar::Text as Words;\nfn f(x: &Scalar) -> bool { matches!(x, Words(_)) }";
    assert_eq!(
        sites(OUTSIDE, source),
        vec!["crates/andromeda-subscription/src/matching.rs:3: matches on `Scalar::Words`".to_owned()]
    );
}

#[test]
fn an_imported_variant_inside_matches_is_reported() {
    let source = "use SetMembership::In;\nfn f(x: &SetMembership) -> bool { matches!(x, In { .. }) }";
    assert_eq!(sites(OUTSIDE, source).len(), 1);
}

#[test]
fn a_renamed_watched_type_is_read_as_the_type() {
    let source = "use crate::filter::OrderingOp as Operator;\nfn f(op: Operator) -> bool { match op { Operator::Eq => true, other => false } }";
    assert_eq!(sites(OUTSIDE, source).len(), 1);
}

#[test]
fn a_glob_import_of_a_watched_type_is_reported_at_the_import() {
    let source = "use crate::filter::SetMembership::*;\nfn f() {}";
    assert_eq!(
        sites(OUTSIDE, source),
        vec!["crates/andromeda-subscription/src/matching.rs:1: imports every variant of `SetMembership`".to_owned()]
    );
}

#[test]
fn importing_the_type_or_a_variant_for_construction_only_is_not_reported() {
    let source = "use crate::filter::{OrderingOp, OrderingOp::Eq};\nfn f() -> OrderingOp { Eq }";
    assert_eq!(sites(OUTSIDE, source), Vec::<String>::new());
}

#[test]
fn an_import_inside_a_test_module_does_not_make_production_names_variants() {
    let source = "#[cfg(test)]\nmod tests { use OrderingOp::Eq; }\nfn f(x: Y) -> bool { match x { Eq => true, other => false } }";
    assert_eq!(sites(OUTSIDE, source), Vec::<String>::new());
}

#[test]
fn the_owner_directory_and_test_files_are_not_scanned() {
    assert!(!is_scanned("crates/andromeda-ngsi-ld-query/src/filter/eval.rs"));
    assert!(!is_scanned("crates/andromeda-subscription/src/matching_tests.rs"));
    assert!(is_scanned("crates/andromeda-ngsi-ld-query/src/split_filter.rs"));
    assert!(is_scanned(OUTSIDE));
}
