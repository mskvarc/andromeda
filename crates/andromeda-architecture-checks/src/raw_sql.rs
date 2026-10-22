//! The workspace-wide ban on SQL outside the sea-query adapter.
//!
//! The PostgreSQL adapter crate is the one place SQL is spelled and the one
//! place statement text reaches the driver: its statement and command traits
//! are sealed and render privately, so every other crate holds statement
//! values only. This guard keeps it that way, over the parsed syntax tree of
//! every workspace source, in two passes.
//!
//! Every source, test code included, is checked for the ways statement text
//! reaches the driver: a `sqlx` statement constructor, `AssertSqlSafe`,
//! `sqlx`'s own query builder or `raw_sql`, the `sea-query-sqlx` binder, a
//! method taking or producing statement text, an executor method given a
//! string, and the sea-query escape hatches that spell a fragment by hand.
//! Production sources are checked besides for SQL text itself: a `&str`
//! constant or a formatting template that reads as SQL, and a statement
//! rendered to text.
//!
//! There is no baseline and no sanctioned path but the adapter: a site
//! anywhere else fails the gate.

use crate::{
    raw_sql_shape::RawSqlShape,
    source_scan::{collect_use_paths, every_workspace_source, is_test_only, item_is_test_only, workspace_sources},
    sql::looks_like_sql,
};
use proc_macro2::{Ident, Span};
use std::path::Path;
use syn::{
    Expr,
    ExprLit,
    ExprMethodCall,
    Item,
    ItemConst,
    ItemStatic,
    ItemUse,
    Lit,
    Macro,
    Type,
    visit::{Visit, visit_expr_method_call, visit_item, visit_item_const, visit_item_static, visit_item_use, visit_macro, visit_path},
};

/// The path prefixes where SQL is spelled on purpose: the adapter itself, and
/// this crate, whose detectors and fixtures name the shapes they look for.
const SANCTIONED_PREFIXES: &[&str] = &["crates/andromeda-postgresql-seaquery/", "crates/andromeda-architecture-checks/"];

/// The `sqlx` items that take statement text.
const DRIVER_TEXT_ITEMS: &[&str] = &[
    "AssertSqlSafe",
    "QueryBuilder",
    "query",
    "query_as",
    "query_as_with",
    "query_file",
    "query_file_as",
    "query_scalar",
    "query_scalar_with",
    "query_unchecked",
    "query_with",
    "raw_sql",
];

/// The crates whose every item binds or builds statement text outside the
/// adapter.
const DRIVER_TEXT_CRATES: &[&str] = &["sea_query_sqlx", "sqlx_core"];

/// The sea-query escape hatches that spell SQL by hand.
const CUSTOM_SQL_ITEMS: &[&str] = &["cust", "cust_with_expr", "cust_with_exprs", "cust_with_values", "raw_query", "raw_sql"];

/// The methods that take or produce statement text.
const TEXT_METHODS: &[&str] = &["begin_with", "build_any_sqlx", "build_sqlx", "copy_in_raw"];

/// The executor methods, which run a string handed to them as statement text.
const EXECUTOR_METHODS: &[&str] = &[
    "describe",
    "execute",
    "execute_many",
    "fetch",
    "fetch_all",
    "fetch_many",
    "fetch_one",
    "fetch_optional",
    "prepare",
    "prepare_with",
];

/// The formatting macros a SQL string is assembled with.
const SQL_FORMATTING_MACROS: &[&str] = &["concat", "format", "write", "writeln"];

/// A site that spells SQL outside the adapter, named by the file it lives in
/// and what shape tripped the guard.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RawSqlSite {
    /// Workspace-relative path of the file.
    pub path: String,
    /// The offending shape; its `Display` is the one-line description.
    pub detail: RawSqlShape,
}

/// Every raw-SQL site in the workspace outside the sanctioned paths, sorted.
#[must_use]
pub fn raw_sql_sites(workspace_root: &str) -> Vec<RawSqlSite> {
    let root = Path::new(workspace_root);
    let mut sites = Vec::new();
    for source in every_workspace_source(root) {
        let mut visitor = DriverTextVisitor {
            path: &source.workspace_path,
            sites: Vec::new(),
        };
        visitor.visit_file(&source.syntax);
        sites.extend(visitor.sites);
    }
    for source in workspace_sources(root) {
        let mut visitor = SqlTextVisitor {
            path: &source.workspace_path,
            sites: Vec::new(),
        };
        visitor.visit_file(&source.syntax);
        sites.extend(visitor.sites);
    }
    sites.retain(|site| !is_sanctioned(&site.path));
    sites.sort();
    sites.dedup();
    sites
}

/// Whether a file is one of the sanctioned homes for SQL.
fn is_sanctioned(path: &str) -> bool {
    SANCTIONED_PREFIXES.iter().any(|prefix| path.starts_with(prefix))
}

/// Assert that no source outside the adapter spells SQL or hands statement
/// text to the driver.
///
/// # Panics
///
/// Panics on any raw-SQL site, or when the workspace source tree cannot be
/// read or parsed.
pub fn assert_no_raw_sql(workspace_root: &str) {
    let sites = raw_sql_sites(workspace_root)
        .into_iter()
        .map(|site| format!("{}: {}", site.path, site.detail))
        .collect::<Vec<_>>();
    assert!(
        sites.is_empty(),
        "SQL outside the sea-query adapter; build these as statements of andromeda-postgresql-seaquery:\n{}",
        sites.join("\n")
    );
}

/// An identifier for a site that names no source identifier of its own.
fn reported(name: &str) -> Ident {
    Ident::new(name, Span::call_site())
}

/// Collects the sites that hand statement text to the driver in one source,
/// test code included.
struct DriverTextVisitor<'path> {
    /// Workspace-relative path currently scanned.
    path: &'path str,
    /// Sites found in the file.
    sites: Vec<RawSqlSite>,
}

impl DriverTextVisitor<'_> {
    /// Record one site.
    fn record(&mut self, detail: RawSqlShape) {
        self.sites.push(RawSqlSite {
            path: self.path.to_owned(),
            detail,
        });
    }

    /// Record the shape a path's segments name, if any.
    fn check_segments(&mut self, segments: &[String]) {
        let Some(last) = segments.last() else {
            return;
        };
        let names = |crate_name: &str| segments.first().is_some_and(|first| first == crate_name);
        let driver_text = DRIVER_TEXT_CRATES.iter().any(|crate_name| names(crate_name))
            || segments.iter().any(|segment| segment == "SqlxBinder")
            || (names("sqlx") && DRIVER_TEXT_ITEMS.contains(&last.as_str()));
        let custom_fragment = (segments.len() > 1 && CUSTOM_SQL_ITEMS.contains(&last.as_str()))
            || (last == "Custom" && segments.iter().rev().nth(1).is_some_and(|owner| owner == "Expr" || owner == "SimpleExpr"));
        if driver_text {
            self.record(RawSqlShape::DriverText { name: reported(last) });
        } else if custom_fragment {
            self.record(RawSqlShape::CustomFragment { name: reported(last) });
        }
    }
}

impl<'ast> Visit<'ast> for DriverTextVisitor<'_> {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        let segments = path.segments.iter().map(|segment| segment.ident.to_string()).collect::<Vec<_>>();
        self.check_segments(&segments);
        visit_path(self, path);
    }

    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        let mut imported = Vec::new();
        collect_use_paths(&item.tree, &mut Vec::new(), &mut |segments: &[String]| imported.push(segments.to_vec()));
        for segments in imported {
            self.check_segments(&segments);
        }
        visit_item_use(self, item);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        let method = call.method.to_string();
        if TEXT_METHODS.contains(&method.as_str()) || (EXECUTOR_METHODS.contains(&method.as_str()) && call.args.first().is_some_and(is_statement_text)) {
            // The site outlives the parsed tree, so the identifier is owned.
            self.record(RawSqlShape::TextMethod { method: call.method.clone() });
        }
        visit_expr_method_call(self, call);
    }
}

/// Whether an argument is statement text: a string literal, or a constant,
/// which is how a `&'static str` statement is named.
fn is_statement_text(argument: &Expr) -> bool {
    // `syn::Expr` is non-exhaustive, so its shapes are tested one by one
    // rather than matched with a catch-all arm.
    if let Expr::Lit(ExprLit { lit: Lit::Str(_), .. }) = argument {
        return true;
    }
    if let Expr::Reference(reference) = argument {
        return is_statement_text(&reference.expr);
    }
    if let Expr::Path(path) = argument {
        return path.path.segments.last().is_some_and(|segment| {
            let name = segment.ident.to_string();
            name.chars().any(char::is_alphabetic)
                && name
                    .chars()
                    .all(|character| character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_')
        });
    }
    false
}

/// Collects the SQL text one production source spells.
struct SqlTextVisitor<'path> {
    /// Workspace-relative path currently scanned.
    path: &'path str,
    /// Sites found in the file.
    sites: Vec<RawSqlSite>,
}

impl SqlTextVisitor<'_> {
    /// Record one site.
    fn record(&mut self, detail: RawSqlShape) {
        self.sites.push(RawSqlSite {
            path: self.path.to_owned(),
            detail,
        });
    }
}

impl<'ast> Visit<'ast> for SqlTextVisitor<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_is_test_only(item) {
            return;
        }
        visit_item(self, item);
    }

    fn visit_item_const(&mut self, item: &'ast ItemConst) {
        if !is_test_only(&item.attrs) && str_constant_is_sql(&item.ty, &item.expr) {
            // The site outlives the parsed tree, so the identifier is owned.
            self.record(RawSqlShape::Constant { name: item.ident.clone() });
        }
        visit_item_const(self, item);
    }

    fn visit_item_static(&mut self, item: &'ast ItemStatic) {
        if !is_test_only(&item.attrs) && str_constant_is_sql(&item.ty, &item.expr) {
            // The site outlives the parsed tree, so the identifier is owned.
            self.record(RawSqlShape::Static { name: item.ident.clone() });
        }
        visit_item_static(self, item);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        if let Some(segment) = path.segments.last()
            && segment.ident == "PostgresQueryBuilder"
        {
            // The site outlives the parsed tree, so the identifier is owned.
            self.record(RawSqlShape::Rendering { name: segment.ident.clone() });
        }
        visit_path(self, path);
    }

    fn visit_macro(&mut self, mac: &'ast Macro) {
        if let Some(name) = mac.path.segments.last().map(|segment| segment.ident.clone())
            && SQL_FORMATTING_MACROS.iter().any(|candidate| name == candidate)
            && first_string_literal(mac).is_some_and(|literal| looks_like_sql(&literal))
        {
            self.record(RawSqlShape::AssembledString { name });
        }
        visit_macro(self, mac);
    }
}

/// Whether a `&str` constant or static holds a body that reads as SQL.
fn str_constant_is_sql(ty: &Type, expression: &Expr) -> bool {
    if !is_str_reference(ty) {
        return false;
    }
    if let Expr::Lit(ExprLit { lit: Lit::Str(value), .. }) = expression {
        looks_like_sql(&value.value())
    } else {
        false
    }
}

/// Whether a type is exactly a shared string slice.
fn is_str_reference(ty: &Type) -> bool {
    matches!(ty, Type::Reference(reference) if matches!(reference.elem.as_ref(), Type::Path(path) if path.path.is_ident("str")))
}

/// The first string literal in a macro invocation's tokens, unescaped.
fn first_string_literal(mac: &Macro) -> Option<String> {
    mac.tokens.clone().into_iter().find_map(|tree| {
        if let proc_macro2::TokenTree::Literal(literal) = tree
            && let Lit::Str(value) = Lit::new(literal)
        {
            return Some(value.value());
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use crate::raw_sql::{DriverTextVisitor, SqlTextVisitor, is_sanctioned};
    use pretty_assertions::assert_eq;
    use syn::visit::Visit;

    fn driver_text(source: &str) -> Vec<String> {
        let syntax = syn::parse_file(source).unwrap_or_else(|error| panic!("fixture parses: {error}"));
        let mut visitor = DriverTextVisitor {
            path: "fixture.rs",
            sites: Vec::new(),
        };
        visitor.visit_file(&syntax);
        visitor.sites.into_iter().map(|site| site.detail.to_string()).collect()
    }

    fn sql_text(source: &str) -> Vec<String> {
        let syntax = syn::parse_file(source).unwrap_or_else(|error| panic!("fixture parses: {error}"));
        let mut visitor = SqlTextVisitor {
            path: "fixture.rs",
            sites: Vec::new(),
        };
        visitor.visit_file(&syntax);
        visitor.sites.into_iter().map(|site| site.detail.to_string()).collect()
    }

    #[test]
    fn every_sqlx_statement_constructor_is_a_violation_whatever_its_argument() {
        assert_eq!(
            driver_text("fn run() { sqlx::query(\"SELECT id FROM entities\"); }"),
            ["query hands statement text to the driver"]
        );
        assert_eq!(
            driver_text("fn run() { sqlx::query_with(&sql, values); }"),
            ["query_with hands statement text to the driver"]
        );
        assert_eq!(driver_text("fn run() { sqlx::query_scalar(SQL); }").len(), 1);
    }

    #[test]
    fn asserting_text_safe_is_a_violation_in_test_code_too() {
        assert_eq!(
            driver_text("#[cfg(test)] mod tests { fn run() { sqlx::query(sqlx::AssertSqlSafe(sql)); } }").len(),
            2
        );
    }

    #[test]
    fn an_imported_driver_item_is_a_violation() {
        assert_eq!(
            driver_text("use sqlx::{AssertSqlSafe, PgPool};"),
            ["AssertSqlSafe hands statement text to the driver"]
        );
        assert_eq!(driver_text("use sea_query_sqlx::SqlxBinder;").len(), 1);
    }

    #[test]
    fn an_executor_given_a_string_runs_statement_text() {
        assert_eq!(driver_text("fn run() { pool.execute(\"VACUUM\"); }"), ["execute runs statement text"]);
        assert_eq!(driver_text("fn run() { connection.fetch_all(LISTING); }").len(), 1);
        assert_eq!(driver_text("fn run() { statement.to_query().execute(&pool); }"), [] as [String; 0]);
    }

    #[test]
    fn a_text_method_is_a_violation() {
        assert_eq!(driver_text("fn run() { pool.begin_with(text); }"), ["begin_with runs statement text"]);
        assert_eq!(driver_text("fn run() { statement.build_sqlx(builder); }").len(), 1);
    }

    #[test]
    fn a_cust_expression_is_a_violation() {
        assert_eq!(driver_text("fn run() { Expr::cust_with_exprs(\"$1 ? $2\", args); }").len(), 1);
        assert_eq!(driver_text("fn run() { Func::cust(name); }").len(), 1);
        assert_eq!(driver_text("fn run() { let custom = SimpleExpr::Custom(text); }").len(), 1);
    }

    #[test]
    fn a_format_that_assembles_sql_is_a_violation() {
        assert_eq!(
            sql_text("fn run() { let s = format!(\"SELECT {a} FROM t\"); }"),
            ["format! assembles a SQL string"]
        );
        assert_eq!(sql_text("fn run() { let s = format!(\"hello {name}\"); }"), [] as [String; 0]);
    }

    #[test]
    fn a_sql_string_constant_is_a_violation() {
        assert_eq!(sql_text("const UPSERT: &str = \"INSERT INTO t SELECT 1\";"), ["constant UPSERT holds SQL text"]);
    }

    #[test]
    fn production_code_rendering_a_statement_is_a_violation() {
        assert_eq!(
            sql_text("fn run() { let text = select.to_string(PostgresQueryBuilder); }"),
            ["PostgresQueryBuilder renders a statement to text"]
        );
    }

    #[test]
    fn test_code_may_render_a_statement_and_spell_its_expected_text() {
        assert_eq!(
            sql_text("#[cfg(test)] mod tests { fn run() { assert_eq!(select.to_string(PostgresQueryBuilder), \"SELECT 1 FROM t\"); } }"),
            [] as [String; 0]
        );
    }

    #[test]
    fn only_the_adapter_and_this_crate_are_sanctioned() {
        assert!(is_sanctioned("crates/andromeda-postgresql-seaquery/src/cast.rs"));
        assert!(!is_sanctioned("crates/andromeda-postgresql-schema/src/current.rs"));
        assert!(!is_sanctioned("crates/andromeda-postgresql-context-store/src/schema_ddl/database.rs"));
        assert!(!is_sanctioned("tests/tenant_schema/main.rs"));
    }
}
