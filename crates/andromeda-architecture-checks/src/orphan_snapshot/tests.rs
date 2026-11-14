use crate::orphan_snapshot::{assert_every_snapshot_is_referenced, crate_directory, name_pattern::NamePattern};
use pretty_assertions::assert_eq;
use std::{env, fs, panic, path::Path, process};

fn pattern_matches(format: &str, name: &str) -> bool {
    NamePattern::parse(format).matches(name)
}

/// A scratch workspace holding one crate with `source` as its library and a
/// `snapshots/` directory with a file per name in `snapshots`.
fn workspace(label: &str, source: &str, snapshots: &[&str]) -> String {
    let root = env::temp_dir().join(format!("andromeda-orphan-snapshot-{}-{label}", process::id()));
    let package = root.join("crates/sample");
    fs::create_dir_all(package.join("src")).unwrap_or_else(|error| panic!("{error}"));
    fs::create_dir_all(package.join("snapshots")).unwrap_or_else(|error| panic!("{error}"));
    fs::write(package.join("src/lib.rs"), source).unwrap_or_else(|error| panic!("{error}"));
    for name in snapshots {
        fs::write(package.join("snapshots").join(format!("{name}.snap")), "---\n---\nbody\n").unwrap_or_else(|error| panic!("{error}"));
    }
    root.to_string_lossy().into_owned()
}

fn verdict(root: &str) -> Result<(), String> {
    panic::catch_unwind(|| assert_every_snapshot_is_referenced(root)).map_err(|payload| {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|text| (*text).to_owned()))
            .unwrap_or_default()
    })
}

#[test]
fn a_literal_name_matches_only_itself() {
    assert!(pattern_matches("a-b", "a-b"));
    assert!(!pattern_matches("a-b", "a-b-c"));
    assert!(!pattern_matches("a-b", "a"));
}

#[test]
fn a_placeholder_stands_for_any_text() {
    assert!(pattern_matches("refused-{name}", "refused-c-12"));
    assert!(pattern_matches("a-{}-b-{}", "a-1-b-2"));
    assert!(pattern_matches("a-{}-b-{}", "a-1-b-"));
    assert!(!pattern_matches("a-{}-b-{}", "a-1-c-2"));
    assert!(!pattern_matches("refused-{name}", "current-c-12"));
}

#[test]
fn a_placeholder_format_spec_is_still_a_placeholder() {
    assert!(pattern_matches("step-{index:03}", "step-007"));
}

#[test]
fn doubled_braces_are_the_braces_themselves() {
    assert!(pattern_matches("set-{{a}}", "set-{a}"));
    assert!(!pattern_matches("set-{{a}}", "set-b"));
}

#[test]
fn the_crate_of_a_source_is_its_package_directory() {
    assert_eq!(crate_directory("crates/sample/src/lib.rs"), "crates/sample");
    assert_eq!(crate_directory("tools/runner/tests/a.rs"), "tools/runner");
    assert_eq!(crate_directory("src/main.rs"), "");
}

#[test]
fn a_snapshot_a_literal_name_takes_is_referenced() {
    let root = workspace(
        "literal",
        "#[test]\nfn a() { assert_statement_snapshot!(\"alpha\", statement()); }\n",
        &["alpha"],
    );
    assert_eq!(verdict(&root), Ok(()));
}

#[test]
fn a_snapshot_no_test_takes_is_reported_by_path() {
    let root = workspace(
        "orphan",
        "#[test]\nfn a() { assert_statement_snapshot!(\"alpha\", statement()); }\n",
        &["alpha", "beta"],
    );
    let failure = verdict(&root).expect_err("beta is taken by no test");
    assert!(failure.contains("crates/sample/snapshots/beta.snap"), "{failure}");
    assert!(!failure.contains("alpha.snap"), "{failure}");
}

#[test]
fn a_snapshot_a_named_snapshot_assertion_takes_is_referenced() {
    let root = workspace("named", "#[test]\nfn a() { assert_named_snapshot!(\"alpha\", text()); }\n", &["alpha"]);
    assert_eq!(verdict(&root), Ok(()));
}

#[test]
fn a_snapshot_a_format_name_can_produce_is_referenced() {
    let source = "#[test]\nfn a() { for n in names() { assert_statement_snapshot!(format!(\"current-{n}\"), statement(n)); } }\n";
    let root = workspace("format", source, &["current-c-1", "current-c-2"]);
    assert_eq!(verdict(&root), Ok(()));
}

#[test]
fn a_snapshot_a_local_bound_to_a_format_takes_is_referenced() {
    let source = "#[test]\nfn a() {\n    let name = format!(\"grid-{}\", 1);\n    assert_statement_snapshot!(name, statement());\n}\n";
    let root = workspace("local", source, &["grid-1", "other-1"]);
    let failure = verdict(&root).expect_err("other-1 is taken by no test");
    assert!(failure.contains("other-1.snap"), "{failure}");
    assert!(!failure.contains("grid-1.snap"), "{failure}");
}

#[test]
fn an_assertion_nested_in_another_macro_is_read() {
    let source = "fn a() { cases!(assert_statement_snapshot!(\"alpha\", statement())); }\n";
    let root = workspace("nested", source, &["alpha"]);
    assert_eq!(verdict(&root), Ok(()));
}

#[test]
fn a_name_the_gate_cannot_read_is_reported_with_its_line() {
    let source = "fn a() {\n    assert_statement_snapshot!(name_of(1), statement());\n}\n";
    let root = workspace("unreadable", source, &["alpha"]);
    let failure = verdict(&root).expect_err("the name cannot be read");
    assert!(failure.contains("crates/sample/src/lib.rs:2"), "{failure}");
}

#[test]
fn the_definition_of_an_assertion_macro_is_not_an_assertion() {
    let source = "macro_rules! assert_statement_snapshot { ($name:expr, $s:expr) => { $crate::assert_named_snapshot!($name, $s) }; }\n";
    let root = workspace("definition", source, &[]);
    assert_eq!(verdict(&root), Ok(()));
}

#[test]
fn a_snapshot_directory_with_no_assertion_in_its_crate_is_all_orphans() {
    let root = workspace("empty", "pub fn a() {}\n", &["alpha"]);
    assert!(Path::new(&root).join("crates/sample/snapshots/alpha.snap").exists());
    let failure = verdict(&root).expect_err("alpha is taken by no test");
    assert!(failure.contains("alpha.snap"), "{failure}");
}
