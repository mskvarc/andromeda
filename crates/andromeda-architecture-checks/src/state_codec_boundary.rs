//! The ban on the expanded entity codec inside the PostgreSQL crates.
//!
//! PostgreSQL holds an Entity's state as typed rows and records only: current
//! state, the queues and the history assemble an `EntityState` from the same
//! flattened instance form, and no relation holds it as JSON text. The expanded
//! codec (`EntityState::from_slice`, `to_vec`, `to_writer`) turns a state into
//! that text and back, so a call to it from a PostgreSQL crate is a relation
//! about to hold the document again.
//!
//! A syntax tree carries no receiver types, so a call is recognised by its
//! shape: a path naming one of the three functions on `EntityState`, any
//! `.to_writer(..)` method call, and a `.to_vec()` whose result is read as a
//! `Result` (the codec's encoding is fallible; a slice copy is not).
//!
//! Detection is deny-by-default and runs at zero. Integration tests, build
//! scripts, `*tests.rs` files and test-only items are not production paths and
//! are not inspected; fixtures keep decoding stored documents.

use crate::source_scan::{item_is_test_only, workspace_sources};
use std::path::Path;
use syn::{
    Expr,
    ExprMethodCall,
    Item,
    spanned::Spanned,
    visit::{Visit, visit_expr, visit_item},
};

/// Workspace path prefix of every crate the ban covers.
const POSTGRESQL_CRATES: &str = "crates/andromeda-postgresql-";

/// The codec functions `EntityState` carries.
const CODEC_FUNCTIONS: [&str; 3] = ["from_slice", "to_vec", "to_writer"];

/// The methods that read a value as a `Result`, so a `.to_vec()` they follow is
/// the fallible codec encoding.
const RESULT_READERS: [&str; 13] = [
    "and_then",
    "err",
    "expect",
    "expect_err",
    "inspect_err",
    "is_err",
    "is_ok",
    "map_err",
    "ok",
    "or_else",
    "unwrap",
    "unwrap_or_default",
    "unwrap_or_else",
];

/// One production call of the expanded entity codec in a PostgreSQL crate.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct StateCodecSite {
    /// Workspace-relative path of the file the call lives in.
    pub path: String,
    /// One-based line the call starts on.
    pub line: usize,
    /// The call's shape, `EntityState::from_slice` or `.to_vec()` for instance.
    pub subject: String,
}

/// Every production call of the expanded entity codec in a PostgreSQL crate,
/// sorted by path and line.
///
/// # Panics
///
/// Panics when the workspace source tree cannot be read or parsed.
#[must_use]
pub fn state_codec_sites(workspace_root: &str) -> Vec<StateCodecSite> {
    let mut sites: Vec<StateCodecSite> = workspace_sources(Path::new(workspace_root))
        .iter()
        .flat_map(|source| source_sites(&source.workspace_path, &source.syntax))
        .collect();
    sites.sort();
    sites
}

/// The codec calls of one parsed source at `path`, in source order; none when
/// the file is no production file of a PostgreSQL crate.
fn source_sites(path: &str, syntax: &syn::File) -> Vec<StateCodecSite> {
    if !path.starts_with(POSTGRESQL_CRATES) || path.contains("/tests/") || path.ends_with("build.rs") || path.ends_with("tests.rs") {
        return Vec::new();
    }
    let mut visitor = StateCodecVisitor { path, sites: Vec::new() };
    visitor.visit_file(syntax);
    visitor.sites
}

/// Assert that no PostgreSQL crate calls the expanded entity codec.
///
/// # Panics
///
/// Panics on any production call, or when the workspace source tree cannot be
/// read or parsed.
pub fn assert_no_state_codec_calls(workspace_root: &str) {
    let sites = state_codec_sites(workspace_root);
    let reports = sites
        .iter()
        .map(|site| format!("  {}:{}: {}", site.path, site.line, site.subject))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        sites.is_empty(),
        "these PostgreSQL sites encode or decode an EntityState as its expanded document; store and read it through the typed state records:\n{reports}"
    );
}

/// Collects the codec calls of one parsed source outside its test-only items.
struct StateCodecVisitor<'path> {
    /// Workspace-relative path every site is reported under.
    path: &'path str,
    /// The calls found so far, in source order.
    sites: Vec<StateCodecSite>,
}

impl StateCodecVisitor<'_> {
    /// Record a call of shape `subject` spanning `node`.
    fn record(&mut self, node: &impl Spanned, subject: String) {
        self.sites.push(StateCodecSite {
            path: self.path.to_owned(),
            line: node.span().start().line,
            subject,
        });
    }
}

impl<'ast> Visit<'ast> for StateCodecVisitor<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_is_test_only(item) {
            return;
        }
        visit_item(self, item);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        if let Some(subject) = codec_call(expr) {
            self.record(expr, subject);
        }
        visit_expr(self, expr);
    }
}

/// The shape of the codec call `expr` is, or `None` when it is none.
///
/// `syn::Expr` is `#[non_exhaustive]`, so the shapes are tested one by one
/// rather than matched with a catch-all arm.
fn codec_call(expr: &Expr) -> Option<String> {
    if let Expr::Path(path) = expr {
        let mut segments = path.path.segments.iter().rev();
        let (function, owner) = (segments.next()?, segments.next()?);
        return (owner.ident == "EntityState" && CODEC_FUNCTIONS.iter().any(|name| function.ident == name)).then(|| format!("EntityState::{}", function.ident));
    }
    if let Expr::MethodCall(call) = expr {
        if call.method == "to_writer" {
            return Some(".to_writer(..)".to_owned());
        }
        return (RESULT_READERS.iter().any(|name| call.method == name) && is_bare_to_vec(&call.receiver)).then(|| format!(".to_vec().{}(..)", call.method));
    }
    if let Expr::Try(attempt) = expr {
        return is_bare_to_vec(&attempt.expr).then(|| ".to_vec()?".to_owned());
    }
    None
}

/// Whether `expr` is a `.to_vec()` call taking no argument.
fn is_bare_to_vec(expr: &Expr) -> bool {
    matches!(expr, Expr::MethodCall(ExprMethodCall { method, args, .. }) if method == "to_vec" && args.is_empty())
}

#[cfg(test)]
mod tests {
    use crate::state_codec_boundary::source_sites;
    use pretty_assertions::assert_eq;

    /// A production source of a PostgreSQL crate.
    const INSIDE: &str = "crates/andromeda-postgresql-context-store/src/x.rs";

    fn subjects(path: &str, source: &str) -> Vec<String> {
        let syntax = syn::parse_file(source).unwrap_or_else(|error| panic!("fixture parses: {error}"));
        source_sites(path, &syntax)
            .into_iter()
            .map(|site| format!("{}: {}", site.line, site.subject))
            .collect()
    }

    #[test]
    fn a_state_codec_call_in_a_postgresql_crate_is_a_violation() {
        let source = "fn read(bytes: &[u8]) -> R {\n    let state = EntityState::from_slice(bytes)?;\n    let text = state.to_vec()?;\n    state.to_writer(&mut out).map_err(E::from)?;\n    let again = state.to_vec().map_err(E::from)?;\n    entity::EntityState::to_vec(&state)\n}\n";
        assert_eq!(
            subjects(INSIDE, source),
            vec![
                "2: EntityState::from_slice",
                "3: .to_vec()?",
                "4: .to_writer(..)",
                "5: .to_vec().map_err(..)",
                "6: EntityState::to_vec",
            ]
        );
    }

    #[test]
    fn a_state_codec_call_outside_them_is_not() {
        let source = "fn read(bytes: &[u8]) -> R { EntityState::from_slice(bytes) }\n";
        assert_eq!(subjects("crates/andromeda-entity/src/x.rs", source), Vec::<String>::new());
        assert_eq!(subjects("crates/andromeda-postgresql-context-store/tests/x.rs", source), Vec::<String>::new());
    }

    #[test]
    fn a_test_only_call_is_ignored() {
        let source = "#[cfg(test)]\nmod tests {\n    fn fixture(bytes: &[u8]) -> EntityState { EntityState::from_slice(bytes).expect(\"a state\") }\n}\n";
        assert_eq!(subjects(INSIDE, source), Vec::<String>::new());
    }

    #[test]
    fn an_infallible_slice_copy_is_not_a_codec_call() {
        let source = "fn bind(keys: &[i64], form: &Form) -> R {\n    let keys = keys.to_vec();\n    Ok(Some(encode(form)?.to_vec()))\n}\n";
        assert_eq!(subjects(INSIDE, source), Vec::<String>::new());
    }
}
