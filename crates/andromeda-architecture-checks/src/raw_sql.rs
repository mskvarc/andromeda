//! The workspace-wide ban on hand-written SQL outside the sanctioned backend.
//!
//! The specification-crate guard in [`crate::sql`] reads a crate's own sources
//! for SQL fragments; this guard is its call-shape counterpart, run once over
//! the whole workspace. Text matching cannot tell `sqlx::query("SELECT …")`
//! from `sqlx::query_with(&sql, values)` — one builds a statement from a string,
//! the other executes a statement sea-query already built — so the detection is
//! over the parsed syntax tree: a raw-string `query*` call, an `Expr::cust*` or
//! `Func::cust` outside the extension crate, a `format!` whose template reads as
//! SQL, or a `&str` constant whose body reads as SQL.
//!
//! Detection is deny-by-default, subtracted twice. A sanctioned path — the
//! sea-query extension and iden crates, the binary-COPY modules, the procedural
//! `schema_ddl` — is where dialect SQL is spelled on purpose and is never a
//! violation. A baseline path is a site not yet converted; it is tolerated so
//! the gate passes today and fails on anything new, and each migration phase
//! deletes its converted entries. A baseline entry that no longer violates is
//! itself a failure, so the ratchet can only tighten.

use crate::{
    source_scan::{is_test_only, item_is_test_only, workspace_sources},
    sql::looks_like_sql,
};
use std::{collections::BTreeSet, path::Path};
use syn::{
    Expr,
    ExprCall,
    ExprLit,
    ExprMethodCall,
    Item,
    ItemConst,
    ItemStatic,
    Lit,
    Macro,
    Type,
    visit::{Visit, visit_expr_call, visit_expr_method_call, visit_item, visit_item_const, visit_item_static, visit_macro},
};

/// The path prefixes where hand-written dialect SQL is sanctioned.
///
/// The sea-query extension and the iden catalogue are the backend itself; the
/// COPY modules speak the binary wire protocol sea-query has no form for; a
/// `schema_ddl` module holds the `CREATE`/`TimescaleDB` DDL `SeaQuery` cannot
/// express. The Snapshot branch modules are that same category: a branch is a
/// per-Snapshot PostgreSQL schema, and `CREATE SCHEMA`, `CREATE TABLE ... (LIKE
/// ... INCLUDING ALL)`, `ALTER SCHEMA ... RENAME`, `DROP SCHEMA`, the
/// cross-schema `INSERT ... SELECT` clone over dynamically named schemas, and
/// the `pg_total_relation_size` branch-size sum have no sea-query form.
/// Everything else builds statements through sea-query.
const SANCTIONED_PREFIXES: &[&str] = &[
    "crates/andromeda-postgresql-seaquery/",
    "crates/andromeda-postgresql-schema/",
    "crates/andromeda-postgresql-encoding/src/copy.rs",
    "crates/andromeda-postgresql-context-store/src/entity/copy.rs",
    "crates/andromeda-postgresql-context-store/src/snapshot/branch_schema.rs",
    "crates/andromeda-postgresql-context-store/src/snapshot/clone/",
    "crates/andromeda-postgresql-context-store/src/snapshot/quota.rs",
    "crates/andromeda-postgresql-temporal-store/src/copy.rs",
];

/// The raw-SQL sites present when the gate was introduced.
///
/// Every entry is a file the migration has yet to convert. A new raw-SQL
/// site in any other file fails the gate immediately; each migration phase
/// deletes the entries it converts, and a converted file left here fails the
/// gate too, so the list can only shrink to empty.
pub const RAW_SQL_BASELINE: &[&str] = &[];

/// The associated-function names that build a statement from a raw string.
const RAW_QUERY_FUNCTIONS: &[&str] = &["query", "query_as", "query_scalar", "query_with"];

/// The sea-query escape hatches that spell SQL by hand.
const CUSTOM_SQL_FUNCTIONS: &[&str] = &["cust", "cust_with_exprs", "cust_with_values"];

/// The formatting macros a SQL string is assembled with.
const SQL_FORMATTING_MACROS: &[&str] = &["format", "write", "writeln"];

/// A site that builds SQL by hand, named by the file it lives in and what shape
/// tripped the guard.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RawSqlSite {
    /// Workspace-relative path of the file.
    pub path: String,
    /// One-line description of the offending shape.
    pub detail: String,
}

/// Every raw-SQL site in the workspace, sanctioned paths included, sorted.
#[must_use]
pub fn raw_sql_sites(workspace_root: &str) -> Vec<RawSqlSite> {
    let mut sites = Vec::new();
    for source in workspace_sources(Path::new(workspace_root)) {
        let mut visitor = RawSqlVisitor {
            path: &source.workspace_path,
            sites: Vec::new(),
        };
        visitor.visit_file(&source.syntax);
        sites.extend(visitor.sites);
    }
    sites.sort();
    sites.dedup();
    sites
}

/// Whether a file is one of the sanctioned homes for hand-written SQL.
fn is_sanctioned(path: &str) -> bool {
    SANCTIONED_PREFIXES.iter().any(|prefix| path.starts_with(prefix)) || path.contains("schema_ddl")
}

/// Assert that no unsanctioned, non-baseline file builds SQL by hand.
///
/// The gate fails on a violation in a file that is neither sanctioned nor on the
/// shrinking baseline, and equally on a baseline file that no longer violates —
/// a converted phase that forgets to delete its entry. Both directions keep the
/// baseline honest.
///
/// # Panics
///
/// Panics on a new raw-SQL site, on a stale baseline entry, or when the
/// workspace source tree cannot be read or parsed.
pub fn assert_no_new_raw_sql(workspace_root: &str, baseline: &[&str]) {
    let sites = raw_sql_sites(workspace_root);
    let baseline: BTreeSet<&str> = baseline.iter().copied().collect();

    let mut unexpected = Vec::new();
    let mut violating_paths = BTreeSet::new();
    for site in &sites {
        if is_sanctioned(&site.path) {
            continue;
        }
        violating_paths.insert(site.path.as_str());
        if !baseline.contains(site.path.as_str()) {
            unexpected.push(format!("{}: {}", site.path, site.detail));
        }
    }
    unexpected.sort();

    let stale: Vec<&str> = baseline.iter().copied().filter(|path| !violating_paths.contains(path)).collect();

    assert!(
        unexpected.is_empty(),
        "new raw SQL outside the sanctioned backend; build these through sea-query:\n{}",
        unexpected.join("\n")
    );
    assert!(
        stale.is_empty(),
        "these baseline files no longer contain raw SQL; delete them from the baseline:\n{}",
        stale.join("\n")
    );
}

/// Collects raw-SQL sites in one parsed source file.
struct RawSqlVisitor<'path> {
    /// Workspace-relative path currently scanned.
    path: &'path str,
    /// Sites found in the file.
    sites: Vec<RawSqlSite>,
}

impl RawSqlVisitor<'_> {
    /// Record one site.
    fn record(&mut self, detail: impl Into<String>) {
        self.sites.push(RawSqlSite {
            path: self.path.to_string(),
            detail: detail.into(),
        });
    }
}

impl<'ast> Visit<'ast> for RawSqlVisitor<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_is_test_only(item) {
            return;
        }
        visit_item(self, item);
    }

    fn visit_item_const(&mut self, item: &'ast ItemConst) {
        if !is_test_only(&item.attrs) && str_constant_is_sql(&item.ty, &item.expr) {
            self.record(format!("constant {} holds SQL text", item.ident));
        }
        visit_item_const(self, item);
    }

    fn visit_item_static(&mut self, item: &'ast ItemStatic) {
        if !is_test_only(&item.attrs) && str_constant_is_sql(&item.ty, &item.expr) {
            self.record(format!("static {} holds SQL text", item.ident));
        }
        visit_item_static(self, item);
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Some(function) = call_function_name(&call.func) {
            if CUSTOM_SQL_FUNCTIONS.contains(&function.as_str()) {
                self.record(format!("{function} spells a raw SQL fragment"));
            } else if RAW_QUERY_FUNCTIONS.contains(&function.as_str()) && first_argument_is_raw_sql(call.args.first()) {
                self.record(format!("{function} is built from a raw SQL string"));
            }
        }
        visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        let method = call.method.to_string();
        if RAW_QUERY_FUNCTIONS.contains(&method.as_str()) && first_argument_is_raw_sql(call.args.first()) {
            self.record(format!("{method} is built from a raw SQL string"));
        }
        visit_expr_method_call(self, call);
    }

    fn visit_macro(&mut self, mac: &'ast Macro) {
        if let Some(name) = mac.path.segments.last().map(|segment| segment.ident.to_string())
            && SQL_FORMATTING_MACROS.contains(&name.as_str())
            && first_string_literal(mac).is_some_and(|literal| looks_like_sql(&literal))
        {
            self.record(format!("{name}! assembles a SQL string"));
        }
        visit_macro(self, mac);
    }
}

/// The bare function name a call targets, from the last path segment.
fn call_function_name(func: &Expr) -> Option<String> {
    if let Expr::Path(path) = func {
        path.path.segments.last().map(|segment| segment.ident.to_string())
    } else {
        None
    }
}

/// Whether a `query*` call's first argument is a raw SQL string rather than a
/// statement sea-query built.
///
/// A string literal that reads as SQL, or a reference to a constant whose name
/// ends `_SQL`, is raw. A local built by `build_sqlx` is neither, so the
/// sanctioned execution path passes.
fn first_argument_is_raw_sql(argument: Option<&Expr>) -> bool {
    match argument {
        Some(Expr::Lit(ExprLit { lit: Lit::Str(value), .. })) => looks_like_sql(&value.value()),
        Some(Expr::Reference(reference)) => first_argument_is_raw_sql(Some(&reference.expr)),
        Some(Expr::Path(path)) => path.path.segments.last().is_some_and(|segment| segment.ident.to_string().ends_with("_SQL")),
        Some(_) | None => false,
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
    use crate::raw_sql::{RawSqlVisitor, is_sanctioned};
    use syn::visit::Visit;

    fn details(source: &str) -> Vec<String> {
        let syntax = syn::parse_file(source).unwrap_or_else(|error| panic!("fixture parses: {error}"));
        let mut visitor = RawSqlVisitor {
            path: "fixture.rs",
            sites: Vec::new(),
        };
        visitor.visit_file(&syntax);
        visitor.sites.into_iter().map(|site| site.detail).collect()
    }

    #[test]
    fn a_raw_string_query_is_a_violation() {
        assert_eq!(details("fn run() { sqlx::query(\"SELECT id FROM entities\"); }").len(), 1);
    }

    #[test]
    fn a_built_statement_executed_through_query_with_is_not_a_violation() {
        assert_eq!(details("fn run() { sqlx::query_with(&sql, values); }"), [] as [String; 0]);
    }

    #[test]
    fn a_query_built_from_a_sql_named_constant_is_a_violation() {
        assert_eq!(details("fn run() { sqlx::query(UPSERT_SQL); }").len(), 1);
    }

    #[test]
    fn a_cust_expression_is_a_violation() {
        assert_eq!(details("fn run() { Expr::cust_with_exprs(\"$1 ? $2\", args); }").len(), 1);
        assert_eq!(details("fn run() { Func::cust(name); }").len(), 1);
    }

    #[test]
    fn a_format_that_assembles_sql_is_a_violation() {
        assert_eq!(details("fn run() { let s = format!(\"SELECT {a} FROM t\"); }").len(), 1);
    }

    #[test]
    fn a_format_of_ordinary_text_is_not_a_violation() {
        assert_eq!(details("fn run() { let s = format!(\"hello {name}\"); }"), [] as [String; 0]);
    }

    #[test]
    fn a_sql_string_constant_is_a_violation() {
        assert_eq!(details("const UPSERT_SQL: &str = \"INSERT INTO t SELECT 1\";").len(), 1);
    }

    #[test]
    fn a_test_only_item_is_ignored() {
        assert_eq!(
            details("#[cfg(test)] mod tests { fn run() { sqlx::query(\"SELECT 1 FROM t\"); } }"),
            [] as [String; 0]
        );
    }

    #[test]
    fn the_extension_and_copy_paths_are_sanctioned() {
        assert!(is_sanctioned("crates/andromeda-postgresql-seaquery/src/cast.rs"));
        assert!(is_sanctioned("crates/andromeda-postgresql-schema/src/current.rs"));
        assert!(is_sanctioned("crates/andromeda-postgresql-temporal-store/src/copy.rs"));
        assert!(is_sanctioned("crates/andromeda-postgresql-context-store/src/schema_ddl/hypertable.rs"));
        assert!(!is_sanctioned("crates/andromeda-postgresql-context-store/src/entity/write.rs"));
    }
}
