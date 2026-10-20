//! Machine-checked invariants over the Snapshot suite's executable cases.

use regex::Regex;
use std::collections::BTreeSet;

const MAIN_SOURCE: &str = include_str!("main.rs");
/// Every source file of the suite whose attributes the serial-group check
/// covers, by the module name `main.rs` declares it under. This module is left
/// out: it spells the forbidden attributes in the patterns it checks for.
const EXECUTABLE_SOURCES: [(&str, &str); 17] = [
    ("main", MAIN_SOURCE),
    ("branch_schema", include_str!("branch_schema.rs")),
    ("capture", include_str!("capture.rs")),
    ("census", include_str!("census.rs")),
    ("clone_liveness", include_str!("clone_liveness.rs")),
    ("clone_writes", include_str!("clone_writes.rs")),
    ("core", include_str!("core.rs")),
    ("lifecycle", include_str!("lifecycle.rs")),
    ("model_protocol", include_str!("model_protocol.rs")),
    ("notification_adapter", include_str!("../support/notification_adapter.rs")),
    ("purge", include_str!("purge.rs")),
    ("stress", include_str!("stress.rs")),
    ("subscriptions", include_str!("subscriptions.rs")),
    ("support", include_str!("support.rs")),
    ("temporal", include_str!("temporal.rs")),
    ("tenancy_resilience", include_str!("tenancy_resilience.rs")),
    ("typed_round_trip", include_str!("typed_round_trip.rs")),
];

fn executable_sources() -> String {
    EXECUTABLE_SOURCES.iter().map(|(_, source)| *source).collect::<Vec<_>>().join("\n")
}

#[test]
fn snapshot_tests_use_the_named_serial_group_and_no_ignore_attribute() {
    let sources = executable_sources();
    assert!(!sources.contains("#[ignore"), "Snapshot acceptance tests must never be ignored");
    assert!(!sources.contains("#[serial]\n"), "Snapshot acceptance tests must use #[serial(snapshot)]");
}

/// A module added to the suite is checked like every other: its name must be
/// among [`EXECUTABLE_SOURCES`], or the serial-group check silently skips it.
#[test]
fn every_snapshot_module_is_covered_by_the_serial_group_check() {
    let declared = Regex::new(r"(?m)^(?:pub(?:\(crate\))? )?mod ([a-z_]+);")
        .expect("the module pattern is static and valid")
        .captures_iter(MAIN_SOURCE)
        .filter_map(|declaration| declaration.get(1))
        .map(|name| name.as_str())
        .filter(|name| *name != "coverage")
        .collect::<BTreeSet<_>>();
    let covered = EXECUTABLE_SOURCES
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| *name != "main")
        .collect::<BTreeSet<_>>();
    pretty_assertions::assert_eq!(declared, covered);
}

#[test]
fn acceptance_tests_do_not_seed_branch_or_lifecycle_state_through_sql() {
    for forbidden in ["seed_snapshot_entities", "seed_snapshot_temporal_entities", "force_snapshot_status"] {
        assert!(!MAIN_SOURCE.contains(forbidden), "direct Snapshot test-state helper remains: {forbidden}");
    }
}
