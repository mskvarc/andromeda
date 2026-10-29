//! The workspace rule that every geo-query box test reads the box a stored
//! geometry's spatial index keys.
//!
//! `near` (ETSI GS CIM 009 v1.9.1 clause 4.10) measures along great-circle
//! edges, and a long edge bows past its vertices, so a box test against a bare
//! geometry column, whose box bounds the vertices alone, can discard a
//! geometry the distance keeps. Every stored geometry is therefore indexed by
//! its great-circle reach where it has one, and the one module that renders a
//! box test takes that indexed box as a typed operand, never a column.
//!
//! The guard reads every production source and reports a call of a `PostGIS`
//! box-overlap constructor outside that module and outside the module that
//! defines the constructors. Integration tests, build scripts, `*tests.rs`
//! files and test-only items are not production paths and are not inspected.

use crate::source_scan::{item_is_test_only, workspace_sources};
use std::path::Path;
use syn::{
    Expr,
    ExprCall,
    Item,
    visit::{Visit, visit_expr_call, visit_item},
};

/// The box-overlap constructors a box test is built from.
const BOX_TESTS: [&str; 2] = ["bounding_boxes_overlap", "unindexed_bounding_boxes_overlap"];

/// The sources that may call a box-overlap constructor: the one module that
/// renders every geo-query box test, and the module defining the
/// constructors.
const BOX_TEST_OWNERS: [&str; 2] = [
    "crates/andromeda-postgresql-query/src/stored_geometry.rs",
    "crates/andromeda-postgresql-seaquery/src/postgis.rs",
];

/// One production site that builds a box test outside its owner.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct GeometryBoxTestSite {
    /// Workspace-relative path of the file the site lives in.
    pub path: String,
    /// One-based line the constructor is called on.
    pub line: usize,
    /// The constructor called.
    pub function: String,
}

impl GeometryBoxTestSite {
    /// The single-line report for this site, `path:line: function`.
    #[must_use]
    pub fn report(&self) -> String {
        format!("{}:{}: `{}` outside the stored geometry box test", self.path, self.line, self.function)
    }
}

/// Every production site that builds a geo-query box test outside its owner,
/// sorted by path and line.
///
/// # Panics
///
/// Panics when the workspace source tree cannot be read or parsed.
#[must_use]
pub fn geometry_box_test_sites(workspace_root: &str) -> Vec<GeometryBoxTestSite> {
    let mut sites = Vec::new();
    for source in workspace_sources(Path::new(workspace_root)) {
        let path = source.workspace_path.as_str();
        if path.contains("/tests/") || path.ends_with("build.rs") || path.ends_with("tests.rs") || BOX_TEST_OWNERS.contains(&path) {
            continue;
        }
        let mut visitor = GeometryBoxTestVisitor { path, sites: Vec::new() };
        visitor.visit_file(&source.syntax);
        sites.extend(visitor.sites);
    }
    sites.sort();
    sites
}

/// Assert that every geo-query box test is built by its owner.
///
/// # Panics
///
/// Panics on any site [`geometry_box_test_sites`] reports, or when the
/// workspace source tree cannot be read or parsed.
pub fn assert_geometry_box_tests_read_the_indexed_box(workspace_root: &str) {
    let sites: Vec<String> = geometry_box_test_sites(workspace_root).iter().map(GeometryBoxTestSite::report).collect();
    assert!(
        sites.is_empty(),
        "a box test built outside `stored_geometry` can read a bare geometry column, which misses a great-circle edge bowing past its vertices; test a `GeometryBox` with `box_test`:\n{}",
        sites.join("\n")
    );
}

/// Collects the box-overlap calls of one parsed source, outside its test-only
/// items.
struct GeometryBoxTestVisitor<'path> {
    /// Workspace-relative file currently scanned.
    path: &'path str,
    /// The sites found in the file.
    sites: Vec<GeometryBoxTestSite>,
}

impl<'ast> Visit<'ast> for GeometryBoxTestVisitor<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_is_test_only(item) {
            return;
        }
        visit_item(self, item);
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Expr::Path(path) = call.func.as_ref()
            && let Some(last) = path.path.segments.last()
        {
            let function = last.ident.to_string();
            if BOX_TESTS.contains(&function.as_str()) {
                self.sites.push(GeometryBoxTestSite {
                    path: self.path.to_owned(),
                    line: last.ident.span().start().line,
                    function,
                });
            }
        }
        visit_expr_call(self, call);
    }
}

#[cfg(test)]
#[expect(
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::geometry_box_test::GeometryBoxTestVisitor;
    use pretty_assertions::assert_eq;
    use syn::visit::Visit;

    fn functions(source: &str) -> Vec<String> {
        let syntax = syn::parse_file(source).unwrap_or_else(|error| panic!("fixture parses: {error}"));
        let mut visitor = GeometryBoxTestVisitor {
            path: "fixture.rs",
            sites: Vec::new(),
        };
        visitor.visit_file(&syntax);
        visitor.sites.into_iter().map(|site| site.function).collect()
    }

    #[test]
    fn a_box_test_on_a_bare_column_is_reported() {
        assert_eq!(
            functions("fn probe() { bounding_boxes_overlap(Expr::col(Ca::ValueGeometry), envelope()); }"),
            ["bounding_boxes_overlap"]
        );
        assert_eq!(
            functions("fn probe() { postgis::unindexed_bounding_boxes_overlap(column(), envelope()); }"),
            ["unindexed_bounding_boxes_overlap"]
        );
    }

    #[test]
    fn a_box_test_through_the_owner_is_not_reported() {
        assert_eq!(
            functions("fn probe() { box_test(&georel, &reference, &stored.indexed_box(), BoxIndexing::Indexed); }"),
            [] as [String; 0]
        );
    }

    #[test]
    fn a_test_only_box_test_is_ignored() {
        assert_eq!(
            functions("#[cfg(test)] mod tests { fn probe() { bounding_boxes_overlap(column(), envelope()); } }"),
            [] as [String; 0]
        );
    }
}
