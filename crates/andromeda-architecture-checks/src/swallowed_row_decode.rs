//! The workspace-wide rule that a row decode failure is propagated, never
//! swallowed.
//!
//! A column read through `sqlx::Row::try_get` fails when the column is missing
//! or its value does not decode as the requested type. Turning that failure
//! into a default, an absent value or a branch not taken reads a damaged or
//! mismatched row as a well-formed one: a total goes missing, a stream looks
//! complete, a count reads zero, and the client is answered as if nothing were
//! wrong. The panicking `Row::get` is barred by the workspace's clippy
//! `disallowed-methods` configuration; this guard bars the quiet form.
//!
//! The guard reads every production source and reports a decode whose result
//! an adapter that discards the error consumes (`ok`, `unwrap_or`,
//! `unwrap_or_default`, `map_or`, `is_ok` and the like), directly or through
//! other `Result` adapters, a decode bound by `if let`, `while let` or
//! `let ... else`, and a decode taken apart by `match`. A decode the `?`
//! operator propagates is never reported, whatever follows it: what follows
//! reads the decoded value, such as a SQL `NULL` read as `None`.
//!
//! Detection is deny-by-default over a baseline that starts empty: a new site
//! fails the gate with its `path:line`, and a baseline file that no longer
//! swallows a decode fails it too, so the list can only shrink. Integration
//! tests, build scripts, `*tests.rs` files, the test-infrastructure crates and
//! test-only items are not production paths and are not inspected.

use crate::source_scan::{item_is_test_only, workspace_sources};
use std::{collections::BTreeSet, path::Path};
use syn::{
    Expr,
    ExprCall,
    ExprLet,
    ExprMatch,
    ExprMethodCall,
    Item,
    Local,
    visit::{Visit, visit_expr_let, visit_expr_match, visit_expr_method_call, visit_item, visit_local},
};

/// The production files that swallowed a row decode failure when the gate was
/// introduced.
pub const SWALLOWED_ROW_DECODE_BASELINE: &[&str] = &[];

/// The `sqlx::Row` methods that decode a column and report its failure.
const DECODE_METHODS: [&str; 3] = ["try_get", "try_get_unchecked", "try_get_raw"];

/// The `Result` adapters that drop the error they are handed.
const DISCARDING_ADAPTERS: [&str; 15] = [
    "err",
    "into_iter",
    "is_err",
    "is_err_and",
    "is_ok",
    "is_ok_and",
    "iter",
    "map_or",
    "map_or_else",
    "ok",
    "or",
    "or_else",
    "unwrap_or",
    "unwrap_or_default",
    "unwrap_or_else",
];

/// One production site that swallows a row decode failure.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SwallowedRowDecodeSite {
    /// Workspace-relative path of the file the site lives in.
    pub path: String,
    /// One-based line the swallowing construct starts on.
    pub line: usize,
    /// The construct that swallows the failure, as written.
    pub construct: String,
}

impl SwallowedRowDecodeSite {
    /// The single-line report for this site, `path:line: construct`.
    #[must_use]
    pub fn report(&self) -> String {
        format!("{}:{}: decode failure swallowed by {}", self.path, self.line, self.construct)
    }
}

/// Every production site that swallows a row decode failure, sorted by path
/// and line.
///
/// # Panics
///
/// Panics when the workspace source tree cannot be read or parsed.
#[must_use]
pub fn swallowed_row_decode_sites(workspace_root: &str) -> Vec<SwallowedRowDecodeSite> {
    let mut sites = Vec::new();
    for source in workspace_sources(Path::new(workspace_root)) {
        if source.workspace_path.contains("/tests/") || source.workspace_path.ends_with("build.rs") || source.workspace_path.ends_with("tests.rs") {
            continue;
        }
        let mut visitor = SwallowedRowDecodeVisitor {
            path: &source.workspace_path,
            sites: Vec::new(),
        };
        visitor.visit_file(&source.syntax);
        sites.extend(visitor.sites);
    }
    sites.sort();
    sites
}

/// Assert that no production source off the baseline swallows a row decode
/// failure.
///
/// # Panics
///
/// Panics on a site in a file off the baseline, on a baseline file that no
/// longer swallows a decode, or when the workspace source tree cannot be read
/// or parsed.
pub fn assert_row_decode_failures_are_propagated(workspace_root: &str, baseline: &[&str]) {
    let sites = swallowed_row_decode_sites(workspace_root);
    let baseline: BTreeSet<&str> = baseline.iter().copied().collect();
    let unexpected: Vec<String> = sites
        .iter()
        .filter(|site| !baseline.contains(site.path.as_str()))
        .map(SwallowedRowDecodeSite::report)
        .collect();
    let swallowing: BTreeSet<&str> = sites.iter().map(|site| site.path.as_str()).collect();
    let stale: Vec<&str> = baseline.iter().copied().filter(|path| !swallowing.contains(path)).collect();

    assert!(
        unexpected.is_empty(),
        "row decode failures swallowed outside the baseline; propagate them with `?` or map them into a typed error:\n{}",
        unexpected.join("\n")
    );
    assert!(
        stale.is_empty(),
        "these baseline files no longer swallow a row decode failure; delete them from the baseline:\n{}",
        stale.join("\n")
    );
}

/// Collects the sites of one parsed source that swallow a row decode failure,
/// outside its test-only items.
struct SwallowedRowDecodeVisitor<'path> {
    /// Workspace-relative file currently scanned.
    path: &'path str,
    /// The sites found in the file.
    sites: Vec<SwallowedRowDecodeSite>,
}

impl SwallowedRowDecodeVisitor<'_> {
    /// Record one site.
    fn record(&mut self, line: usize, construct: String) {
        self.sites.push(SwallowedRowDecodeSite {
            path: self.path.to_owned(),
            line,
            construct,
        });
    }
}

impl<'ast> Visit<'ast> for SwallowedRowDecodeVisitor<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_is_test_only(item) {
            return;
        }
        visit_item(self, item);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        let adapter = call.method.to_string();
        if DISCARDING_ADAPTERS.contains(&adapter.as_str()) && reaches_decode(&call.receiver) {
            self.record(call.method.span().start().line, format!("`{adapter}`"));
        }
        visit_expr_method_call(self, call);
    }

    fn visit_expr_let(&mut self, binding: &'ast ExprLet) {
        if reaches_decode(&binding.expr) {
            self.record(binding.let_token.span.start().line, "`if let` or `while let`".to_owned());
        }
        visit_expr_let(self, binding);
    }

    fn visit_local(&mut self, local: &'ast Local) {
        if let Some(init) = &local.init
            && init.diverge.is_some()
            && reaches_decode(&init.expr)
        {
            self.record(local.let_token.span.start().line, "`let ... else`".to_owned());
        }
        visit_local(self, local);
    }

    fn visit_expr_match(&mut self, matched: &'ast ExprMatch) {
        if reaches_decode(&matched.expr) {
            self.record(matched.match_token.span.start().line, "`match`".to_owned());
        }
        visit_expr_match(self, matched);
    }
}

/// Whether an expression is a row decode, or a chain of method calls whose
/// receiver is one.
///
/// The chain stops at anything that is not a method call, a function call or
/// parentheses, so a decode the `?` operator has already propagated never
/// reaches here. `syn::Expr` is `#[non_exhaustive]`, so the shapes are tested
/// one by one rather than matched.
fn reaches_decode(expression: &Expr) -> bool {
    if let Expr::MethodCall(call) = expression {
        return is_decode_method(call) || reaches_decode(&call.receiver);
    }
    if let Expr::Call(call) = expression {
        return is_decode_function(call);
    }
    if let Expr::Paren(inner) = expression {
        return reaches_decode(&inner.expr);
    }
    false
}

/// Whether a method call decodes a row column.
fn is_decode_method(call: &ExprMethodCall) -> bool {
    DECODE_METHODS.contains(&call.method.to_string().as_str())
}

/// Whether a function call decodes a row column through its trait path, as
/// `Row::try_get(&row, column)`.
fn is_decode_function(call: &ExprCall) -> bool {
    let Expr::Path(function) = call.func.as_ref() else {
        return false;
    };
    function
        .path
        .segments
        .last()
        .is_some_and(|segment| DECODE_METHODS.contains(&segment.ident.to_string().as_str()))
}

#[cfg(test)]
mod tests {
    use crate::swallowed_row_decode::SwallowedRowDecodeVisitor;
    use pretty_assertions::assert_eq;
    use syn::visit::Visit;

    fn constructs(source: &str) -> Vec<String> {
        let syntax = syn::parse_file(source).unwrap_or_else(|error| panic!("fixture parses: {error}"));
        let mut visitor = SwallowedRowDecodeVisitor {
            path: "fixture.rs",
            sites: Vec::new(),
        };
        visitor.visit_file(&syntax);
        visitor.sites.into_iter().map(|site| site.construct).collect()
    }

    #[test]
    fn a_decode_turned_into_an_absent_value_is_swallowed() {
        assert_eq!(
            constructs(r#"fn read() { rows.first().and_then(|r| r.try_get::<i64, _>("count").ok()); }"#),
            ["`ok`"]
        );
    }

    #[test]
    fn a_decode_turned_into_a_default_is_swallowed() {
        assert_eq!(
            constructs(r#"fn read() { row.try_get::<String, _>("database").unwrap_or_default(); }"#),
            ["`unwrap_or_default`"]
        );
        assert_eq!(constructs(r#"fn read() { row.try_get::<i64, _>("n").unwrap_or(0); }"#), ["`unwrap_or`"]);
    }

    #[test]
    fn a_decode_discarded_after_other_adapters_is_swallowed() {
        assert_eq!(constructs(r#"fn read() { row.try_get::<String, _>("n").map(parse).ok(); }"#), ["`ok`"]);
    }

    #[test]
    fn a_decode_spelled_through_its_trait_path_is_seen() {
        assert_eq!(constructs(r#"fn read() { Row::try_get::<i64, _>(&row, "n").ok(); }"#), ["`ok`"]);
    }

    #[test]
    fn a_decode_bound_by_if_let_or_let_else_is_swallowed() {
        assert_eq!(
            constructs(r#"fn read() { if let Ok(value) = row.try_get::<i64, _>("n") { use_it(value); } }"#),
            ["`if let` or `while let`"]
        );
        assert_eq!(
            constructs(r#"fn read() { let Ok(value) = row.try_get::<i64, _>("n") else { return; }; }"#),
            ["`let ... else`"]
        );
    }

    #[test]
    fn a_decode_taken_apart_by_match_is_swallowed() {
        let source = r#"fn read() { match row.try_get::<Option<String>, _>("alias") { Ok(value) => value, Err(_) => None }; }"#;
        assert_eq!(constructs(source), ["`match`"]);
    }

    #[test]
    fn a_propagated_decode_is_not_swallowed_whatever_reads_its_value() {
        assert_eq!(
            constructs(r#"fn read() -> Result<(), E> { row.try_get::<Option<i32>, _>("n")?.unwrap_or_default(); Ok(()) }"#),
            [] as [String; 0]
        );
        assert_eq!(
            constructs(r#"fn read() -> Result<(), E> { row.try_get::<i64, _>("n").map_err(E::from)?; Ok(()) }"#),
            [] as [String; 0]
        );
    }

    #[test]
    fn a_discarding_adapter_on_anything_but_a_decode_is_not_reported() {
        assert_eq!(
            constructs(r"fn read() { text.parse::<i64>().ok(); headers.typed_try_get::<Tenant>().ok(); }"),
            [] as [String; 0]
        );
    }

    #[test]
    fn a_test_only_swallowed_decode_is_ignored() {
        assert_eq!(
            constructs(r#"#[cfg(test)] mod tests { fn read() { row.try_get::<i64, _>("n").ok(); } }"#),
            [] as [String; 0]
        );
    }
}
