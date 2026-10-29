//! The workspace-wide rule that a `dataset_id` column is never compared
//! null-aware.
//!
//! An attribute instance is identified by its Entity, its attribute name and
//! its `datasetId` (ETSI GS CIM 009 v1.9.1 clause 4.5.5), the default instance
//! carrying none. `IS NOT DISTINCT FROM` matches the two absent ones, but
//! PostgreSQL can neither hash, merge nor index it, so a join on it tests
//! every pair of rows: matching the instances of one write against the ones
//! its Entity holds costs their product. The grain indexes key the column as
//! `COALESCE(dataset_id, '')` and every comparison is spelled through the
//! nullable text key helpers, which compare that expression with plain `=`.
//!
//! The guard reads every production source and reports a call of
//! `not_distinct_from` or `distinct_from` whose receiver or argument names a
//! `DatasetId` column. Integration tests, build scripts, `*tests.rs` files and
//! test-only items are not production paths and are not inspected.

use crate::source_scan::{item_is_test_only, workspace_sources};
use proc_macro2::{TokenStream, TokenTree};
use quote::ToTokens;
use std::path::Path;
use syn::{
    ExprMethodCall,
    Item,
    visit::{Visit, visit_expr_method_call, visit_item},
};

/// The null-aware comparison methods a dataset column must not be handed to.
const NULL_AWARE_COMPARISONS: [&str; 2] = ["not_distinct_from", "distinct_from"];

/// The column identifier every relation names its `dataset_id` column by.
const DATASET_COLUMN: &str = "DatasetId";

/// One production site that compares a `dataset_id` column null-aware.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DatasetGrainComparisonSite {
    /// Workspace-relative path of the file the site lives in.
    pub path: String,
    /// One-based line the comparison is called on.
    pub line: usize,
    /// The comparison method called.
    pub method: String,
}

impl DatasetGrainComparisonSite {
    /// The single-line report for this site, `path:line: method`.
    #[must_use]
    pub fn report(&self) -> String {
        format!("{}:{}: `{}` on a dataset_id column", self.path, self.line, self.method)
    }
}

/// Every production site that compares a `dataset_id` column null-aware,
/// sorted by path and line.
///
/// # Panics
///
/// Panics when the workspace source tree cannot be read or parsed.
#[must_use]
pub fn dataset_grain_comparison_sites(workspace_root: &str) -> Vec<DatasetGrainComparisonSite> {
    let mut sites = Vec::new();
    for source in workspace_sources(Path::new(workspace_root)) {
        if source.workspace_path.contains("/tests/") || source.workspace_path.ends_with("build.rs") || source.workspace_path.ends_with("tests.rs") {
            continue;
        }
        let mut visitor = DatasetGrainComparisonVisitor {
            path: &source.workspace_path,
            sites: Vec::new(),
        };
        visitor.visit_file(&source.syntax);
        sites.extend(visitor.sites);
    }
    sites.sort();
    sites
}

/// Assert that no production source compares a `dataset_id` column
/// null-aware.
///
/// # Panics
///
/// Panics on any such site, or when the workspace source tree cannot be read
/// or parsed.
pub fn assert_dataset_columns_compare_by_their_grain_key(workspace_root: &str) {
    let sites: Vec<String> = dataset_grain_comparison_sites(workspace_root)
        .iter()
        .map(DatasetGrainComparisonSite::report)
        .collect();
    assert!(
        sites.is_empty(),
        "dataset_id columns compared null-aware, which PostgreSQL joins pair by pair; compare them with `same_nullable_text_key`:\n{}",
        sites.join("\n")
    );
}

/// Collects the null-aware dataset comparisons of one parsed source, outside
/// its test-only items.
struct DatasetGrainComparisonVisitor<'path> {
    /// Workspace-relative file currently scanned.
    path: &'path str,
    /// The sites found in the file.
    sites: Vec<DatasetGrainComparisonSite>,
}

impl<'ast> Visit<'ast> for DatasetGrainComparisonVisitor<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_is_test_only(item) {
            return;
        }
        visit_item(self, item);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        let method = call.method.to_string();
        let compares_a_dataset =
            names_dataset_column(call.receiver.to_token_stream()) || call.args.iter().any(|argument| names_dataset_column(argument.to_token_stream()));
        if NULL_AWARE_COMPARISONS.contains(&method.as_str()) && compares_a_dataset {
            self.sites.push(DatasetGrainComparisonSite {
                path: self.path.to_owned(),
                line: call.method.span().start().line,
                method,
            });
        }
        visit_expr_method_call(self, call);
    }
}

/// Whether `tokens` name the `DatasetId` column anywhere.
fn names_dataset_column(tokens: TokenStream) -> bool {
    tokens.into_iter().any(|token| match token {
        TokenTree::Ident(ident) => ident == DATASET_COLUMN,
        TokenTree::Group(group) => names_dataset_column(group.stream()),
        TokenTree::Punct(_) | TokenTree::Literal(_) => false,
    })
}

#[cfg(test)]
#[expect(
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::dataset_grain_comparison::DatasetGrainComparisonVisitor;
    use pretty_assertions::assert_eq;
    use syn::visit::Visit;

    fn methods(source: &str) -> Vec<String> {
        let syntax = syn::parse_file(source).unwrap_or_else(|error| panic!("fixture parses: {error}"));
        let mut visitor = DatasetGrainComparisonVisitor {
            path: "fixture.rs",
            sites: Vec::new(),
        };
        visitor.visit_file(&syntax);
        visitor.sites.into_iter().map(|site| site.method).collect()
    }

    #[test]
    fn a_dataset_column_compared_null_aware_is_reported() {
        assert_eq!(
            methods("fn grain() { Expr::col((S, Streams::DatasetId)).not_distinct_from(entry(Entry::DatasetId)); }"),
            ["not_distinct_from"]
        );
        assert_eq!(
            methods("fn grain() { entry(Entry::Kind).distinct_from(stream.column(Grain::DatasetId)); }"),
            ["distinct_from"]
        );
    }

    #[test]
    fn another_column_compared_null_aware_is_not_reported() {
        assert_eq!(
            methods("fn claim() { local(History::VerbatimInstanceId).not_distinct_from(staged(History::VerbatimInstanceId)); }"),
            [] as [String; 0]
        );
    }

    #[test]
    fn a_dataset_column_compared_by_its_grain_key_is_not_reported() {
        assert_eq!(
            methods("fn grain() { same_nullable_text_key(Expr::col((S, Streams::DatasetId)), entry(Entry::DatasetId)); }"),
            [] as [String; 0]
        );
    }

    #[test]
    fn a_test_only_comparison_is_ignored() {
        assert_eq!(
            methods("#[cfg(test)] mod tests { fn grain() { col(Streams::DatasetId).not_distinct_from(other()); } }"),
            [] as [String; 0]
        );
    }
}
