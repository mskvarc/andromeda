//! Machine-checked correspondence between the Snapshot register, catalog, and executable cases.

use regex::Regex;
use std::collections::BTreeSet;

const REGISTER: &str = include_str!("../../docs/snapshot-conformance-register.md");
const CATALOG: &str = include_str!("../../docs/snapshot-tests/README.md");
const MAIN_SOURCE: &str = include_str!("main.rs");
const MODEL_PROTOCOL_SOURCE: &str = include_str!("model_protocol.rs");
const LIFECYCLE_SOURCE: &str = include_str!("lifecycle.rs");
const CAPTURE_SOURCE: &str = include_str!("capture.rs");
const CORE_SOURCE: &str = include_str!("core.rs");
const TEMPORAL_SOURCE: &str = include_str!("temporal.rs");
const SUBSCRIPTIONS_SOURCE: &str = include_str!("subscriptions.rs");
const TENANCY_RESILIENCE_SOURCE: &str = include_str!("tenancy_resilience.rs");
const PURGE_SOURCE: &str = include_str!("purge.rs");
const STRESS_SOURCE: &str = include_str!("stress.rs");
const EXPECTED_REQUIREMENTS: usize = 152;
const EXPECTED_POLICIES: usize = 10;
const EXPECTED_CASES: usize = 76;

fn identifiers(source: &str, pattern: &str) -> BTreeSet<String> {
    Regex::new(pattern)
        .expect("the coverage pattern is static and valid")
        .find_iter(source)
        .map(|matched| matched.as_str().to_owned())
        .collect()
}

fn catalog_cases() -> BTreeSet<String> {
    identifiers(CATALOG, r"snapshot_[a-z0-9_]+")
}

fn executable_sources() -> String {
    [
        MAIN_SOURCE,
        MODEL_PROTOCOL_SOURCE,
        LIFECYCLE_SOURCE,
        CAPTURE_SOURCE,
        CORE_SOURCE,
        TEMPORAL_SOURCE,
        SUBSCRIPTIONS_SOURCE,
        TENANCY_RESILIENCE_SOURCE,
        PURGE_SOURCE,
        STRESS_SOURCE,
    ]
    .join("\n")
}

fn catalog_requirement_identifiers() -> BTreeSet<String> {
    let mut mapped = identifiers(CATALOG, r"REQ-SNAP-[A-Z]+-[0-9]+");
    let ranges = Regex::new(r"(REQ-SNAP-[A-Z]+-)([0-9]+)\.\.([0-9]+)").expect("the requirement-range pattern is static and valid");
    for captures in ranges.captures_iter(CATALOG) {
        let prefix = captures.get(1).expect("the prefix capture is mandatory").as_str();
        let first = captures
            .get(2)
            .expect("the range start capture is mandatory")
            .as_str()
            .parse::<usize>()
            .expect("catalog range starts are integers");
        let last = captures
            .get(3)
            .expect("the range end capture is mandatory")
            .as_str()
            .parse::<usize>()
            .expect("catalog range ends are integers");
        mapped.extend((first..=last).map(|number| format!("{prefix}{number}")));
    }
    mapped
}

#[test]
fn register_has_exact_normative_and_policy_counts() {
    pretty_assertions::assert_eq!(identifiers(REGISTER, r"REQ-SNAP-[A-Z]+-[0-9]+").len(), EXPECTED_REQUIREMENTS);
    pretty_assertions::assert_eq!(identifiers(REGISTER, r"POLICY-SNAP-[0-9]+").len(), EXPECTED_POLICIES);
    let incomplete = REGISTER
        .lines()
        .filter(|line| line.starts_with("[REQ-SNAP-"))
        .filter(|line| !line.ends_with("STATUS: COVERED") && !line.starts_with("[REQ-SNAP-RESULT-5] SPEC-CONFLICT"))
        .collect::<Vec<_>>();
    assert!(
        incomplete.is_empty(),
        "Snapshot obligations with unresolved implementation status: {incomplete:#?}"
    );
    pretty_assertions::assert_eq!(REGISTER.lines().filter(|line| line.ends_with("STATUS: SPEC-CONFLICT")).count(), 1);
}

#[test]
fn every_registered_obligation_is_mapped_by_the_case_catalog() {
    let registered_requirements = identifiers(REGISTER, r"REQ-SNAP-[A-Z]+-[0-9]+");
    let catalog_requirements = catalog_requirement_identifiers();
    let registered_policies = identifiers(REGISTER, r"POLICY-SNAP-[0-9]+");
    let catalog_policies = identifiers(CATALOG, r"POLICY-SNAP-[0-9]+");
    pretty_assertions::assert_eq!(catalog_requirements, registered_requirements);
    pretty_assertions::assert_eq!(catalog_policies, registered_policies);
}

#[test]
fn catalog_has_exactly_seventy_six_unique_executable_case_names() {
    pretty_assertions::assert_eq!(catalog_cases().len(), EXPECTED_CASES);
}

#[test]
fn every_catalog_case_has_an_executable_test_function() {
    let sources = executable_sources();
    let missing = catalog_cases()
        .into_iter()
        .filter(|case| !sources.contains(&format!("fn {case}")))
        .collect::<Vec<_>>();
    assert!(missing.is_empty(), "catalog cases without executable functions: {missing:#?}");
}

#[test]
fn snapshot_tests_use_the_named_serial_group_and_no_ignore_attribute() {
    let sources = executable_sources();
    assert!(!sources.contains("#[ignore"), "Snapshot acceptance tests must never be ignored");
    assert!(!sources.contains("#[serial]\n"), "Snapshot acceptance tests must use #[serial(snapshot)]");
}

#[test]
fn acceptance_tests_do_not_seed_branch_or_lifecycle_state_through_sql() {
    for forbidden in ["seed_snapshot_entities", "seed_snapshot_temporal_entities", "force_snapshot_status"] {
        assert!(!MAIN_SOURCE.contains(forbidden), "direct Snapshot test-state helper remains: {forbidden}");
    }
}
