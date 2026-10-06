use crate::untyped_json::{
    UntypedJsonSite,
    module_imports::{ModuleImports, ModulePosition},
    source_sites,
    violation_report,
};
use pretty_assertions::assert_eq;

/// A production source outside every owner of the processor tree.
const OUTSIDE: &str = "crates/andromeda-notification/src/x.rs";

fn reports(path: &str, source: &str) -> Vec<String> {
    let syntax = parsed(source);
    source_sites(path, &syntax, &ModuleImports::default(), ModulePosition::of(path).as_ref())
        .iter()
        .map(UntypedJsonSite::report)
        .collect()
}

fn parsed(source: &str) -> syn::File {
    syn::parse_file(source).unwrap_or_else(|error| panic!("fixture parses: {error}"))
}

/// The reports of `child` once the imports of every `(path, source)` module are settled.
fn reports_beside(modules: &[(&str, &str)], child: &str) -> Vec<String> {
    let parsed_modules: Vec<(&str, syn::File)> = modules.iter().map(|(path, source)| (*path, parsed(source))).collect();
    let positioned: Vec<_> = parsed_modules.iter().map(|(path, syntax)| (ModulePosition::of(path), syntax)).collect();
    let imports = ModuleImports::settle(&positioned);
    let (_, syntax) = parsed_modules
        .iter()
        .find(|(path, _)| *path == child)
        .unwrap_or_else(|| panic!("{child} is among the modules"));
    source_sites(child, syntax, &imports, ModulePosition::of(child).as_ref())
        .iter()
        .map(UntypedJsonSite::report)
        .collect()
}

fn site(path: &str, line: usize) -> UntypedJsonSite {
    UntypedJsonSite {
        path: path.to_owned(),
        line,
        subject: "serde_json::Value".to_owned(),
    }
}

#[test]
fn a_value_in_production_code_is_reported() {
    let source = "struct Body {\n    payload: serde_json::Value,\n}\n";
    assert_eq!(reports(OUTSIDE, source), vec![format!("{OUTSIDE}:2: serde_json::Value")]);
}

#[test]
fn a_value_in_a_test_module_is_allowed() {
    let source = "#[cfg(test)]\nmod tests {\n    use serde_json::{Value, json};\n    fn fixture() -> Value { json!({}) }\n}\n";
    assert_eq!(reports(OUTSIDE, source), Vec::<String>::new());
}

#[test]
fn a_test_only_impl_member_is_allowed() {
    let source = "impl Body {\n    #[cfg(test)]\n    fn fixture() -> serde_json::Value { todo!() }\n}\n";
    assert_eq!(reports(OUTSIDE, source), Vec::<String>::new());
}

#[test]
fn an_imported_value_counts_at_the_import_and_at_every_use() {
    let source = "use serde_json::{Map, Value};\nfn f(map: Map<String, Value>) -> Value {\n    Value::Object(map)\n}\n";
    assert_eq!(
        reports(OUTSIDE, source),
        vec![
            format!("{OUTSIDE}:1: serde_json::Map"),
            format!("{OUTSIDE}:1: serde_json::Value"),
            format!("{OUTSIDE}:2: Map"),
            format!("{OUTSIDE}:2: Value"),
            format!("{OUTSIDE}:2: Value"),
            format!("{OUTSIDE}:3: Value::Object"),
        ]
    );
}

#[test]
fn a_renamed_import_counts_under_its_local_name() {
    let source = "use serde_json::Value as JsonValue;\nfn f() -> JsonValue { todo!() }\n";
    assert_eq!(
        reports(OUTSIDE, source),
        vec![format!("{OUTSIDE}:1: serde_json::Value"), format!("{OUTSIDE}:2: JsonValue")]
    );
}

#[test]
fn the_json_macro_counts_inside_another_macro() {
    let source = "use serde_json::json;\nfn f() -> Vec<B> {\n    let json = 1;\n    vec![json!({ \"a\": json })]\n}\n";
    assert_eq!(
        reports(OUTSIDE, source),
        vec![format!("{OUTSIDE}:1: serde_json::json"), format!("{OUTSIDE}:4: json")]
    );
}

#[test]
fn tree_conversions_count_and_text_forms_do_not() {
    let source = "fn f(v: &T) -> R {\n    let raw: Box<serde_json::value::RawValue> = serde_json::value::to_raw_value(v)?;\n    let tree = serde_json::to_value(v)?;\n    serde_json::from_slice(b\"1\")\n}\n";
    assert_eq!(reports(OUTSIDE, source), vec![format!("{OUTSIDE}:3: serde_json::to_value")]);
}

#[test]
fn a_tree_named_through_the_value_module_counts() {
    let source = "fn f(map: serde_json::value::Map<K, V>) -> serde_json::value::Value { todo!() }\n";
    assert_eq!(
        reports(OUTSIDE, source),
        vec![format!("{OUTSIDE}:1: serde_json::value::Map"), format!("{OUTSIDE}:1: serde_json::value::Value")]
    );
}

#[test]
fn a_field_or_method_with_a_tree_name_is_not_counted() {
    let source = "use serde_json::Value;\nfn f(body: &B) -> u8 {\n    body.Value.json()\n}\n";
    assert_eq!(reports(OUTSIDE, source), vec![format!("{OUTSIDE}:1: serde_json::Value")]);
}

#[test]
fn a_nested_import_counts_once() {
    let source = "fn f() {\n    use serde_json::Value;\n    let _: Option<Value> = None;\n}\n";
    assert_eq!(
        reports(OUTSIDE, source),
        vec![format!("{OUTSIDE}:2: serde_json::Value"), format!("{OUTSIDE}:3: Value")]
    );
}

#[test]
fn the_processor_tree_is_reported_outside_its_owners() {
    let source = "fn f(v: jsonld_syntax::Value) -> jstrict::Value { todo!() }\n";
    assert_eq!(
        reports(OUTSIDE, source),
        vec![format!("{OUTSIDE}:1: jsonld_syntax::Value"), format!("{OUTSIDE}:1: jstrict::Value")]
    );
}

#[test]
fn the_processor_tree_re_exported_by_the_jsonld_facade_is_reported_too() {
    let source = "fn f(v: jsonld::syntax::Value) {}\n";
    assert_eq!(reports(OUTSIDE, source), vec![format!("{OUTSIDE}:1: jsonld::syntax::Value")]);
    assert_eq!(reports("crates/andromeda-jsonld/src/engine/expansion.rs", source), Vec::<String>::new());
}

#[test]
fn the_processor_tree_is_admitted_in_the_engine_and_the_literal_crate() {
    let source = "fn f(v: jsonld_syntax::Value) -> jstrict::Value { todo!() }\n";
    assert_eq!(reports("crates/andromeda-jsonld/src/engine/expansion.rs", source), Vec::<String>::new());
    assert_eq!(reports("crates/andromeda-json-literal/src/equality.rs", source), Vec::<String>::new());
}

#[test]
fn a_serde_json_tree_is_reported_in_the_processor_owners_too() {
    let source = "fn f() -> serde_json::Value { todo!() }\n";
    assert_eq!(
        reports("crates/andromeda-jsonld/src/engine/expansion.rs", source),
        vec!["crates/andromeda-jsonld/src/engine/expansion.rs:1: serde_json::Value".to_owned()]
    );
}

#[test]
fn a_production_value_fails_the_gate_with_its_file_and_line() {
    let report = violation_report(&[site(OUTSIDE, 7)]).unwrap_or_default();
    assert!(report.ends_with(&format!("\n  {OUTSIDE}:7: serde_json::Value")), "{report}");
}

#[test]
fn every_site_is_reported_on_its_own_line() {
    let report = violation_report(&[site(OUTSIDE, 1), site(OUTSIDE, 2)]).unwrap_or_default();
    assert!(
        report.contains(&format!("\n  {OUTSIDE}:1: serde_json::Value\n  {OUTSIDE}:2: serde_json::Value")),
        "{report}"
    );
}

#[test]
fn a_workspace_without_sites_passes_the_gate() {
    assert_eq!(violation_report(&[]), None);
}

#[test]
fn a_parent_import_reached_through_crate_counts_in_the_child() {
    let parent = "crates/andromeda-http-entity/src/entity.rs";
    let child = "crates/andromeda-http-entity/src/entity/append_attributes.rs";
    let modules = [
        (parent, "use serde_json::Value;\nmod append_attributes;\n"),
        (child, "use crate::entity::{Value, Other};\nfn f() -> Vec<Value> { todo!() }\n"),
    ];
    assert_eq!(
        reports_beside(&modules, child),
        vec![format!("{child}:1: crate::entity::Value"), format!("{child}:2: Value")]
    );
}

#[test]
fn a_parent_import_reached_through_super_counts_in_a_grandchild() {
    let parent = "crates/andromeda-temporal/src/temporal/mod.rs";
    let child = "crates/andromeda-temporal/src/temporal/read.rs";
    let grandchild = "crates/andromeda-temporal/src/temporal/read/split.rs";
    let modules = [
        (parent, "use serde_json::Value as Json;\n"),
        (child, "use super::Json;\n"),
        (grandchild, "use super::super::Json;\nuse super::Json as Again;\n"),
    ];
    assert_eq!(
        reports_beside(&modules, grandchild),
        vec![format!("{grandchild}:1: super::super::Json"), format!("{grandchild}:2: super::Json")]
    );
}

#[test]
fn a_same_named_type_of_another_module_is_not_counted() {
    let parent = "crates/andromeda-entity/src/query.rs";
    let child = "crates/andromeda-entity/src/query/filter.rs";
    let modules = [(parent, "pub struct Value;\n"), (child, "use crate::query::Value;\nfn f(v: Value) {}\n")];
    assert_eq!(reports_beside(&modules, child), Vec::<String>::new());
}
