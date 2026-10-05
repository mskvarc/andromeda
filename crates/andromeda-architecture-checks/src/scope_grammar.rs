//! The workspace rule that one module owns the NGSI-LD Scope grammar.
//!
//! Clause 4.18 writes a Scope as `[%x2F] ScopeLevel *(%x2F ScopeLevel)`. The
//! lexicon's `scope` module is the one reading of it: `ScopePath::from_str`
//! parses a path, `split_levels` reads the optional leading `/`, and
//! `checked_level` admits a level with its storage bound. Every other crate
//! goes through those, so a Scope is read the same way wherever it is written,
//! stored, matched or merged. A second reading -- splitting a Scope on `/` by
//! hand, admitting a level without its bound, or holding the raw grammar type
//! that skips both -- is what let two stores rebuild every Scope with a leading
//! `/` while a third kept it as written.
//!
//! The guard flags, in any production source outside the lexicon's `scope`
//! module:
//!
//! - a path through `scope::grammar`, the raw clause 4.18 ABNF type that
//!   admits the NGSI-LD Null as a Scope and skips the storage bounds;
//! - a call to `ScopeLevel::new` or `ScopeLevelBuf::new`, which admits a level
//!   without `checked_level`'s bound;
//! - in a source that imports a lexicon or query `scope` module, a `split`,
//!   `strip_prefix` or similar call on the `/` separator: that source handles
//!   Scopes, and splitting one is `split_levels`'s job.

use crate::source_scan::{item_is_test_only, workspace_sources};
use std::{collections::BTreeSet, path::Path};
use syn::{
    Expr,
    ExprLit,
    ExprMethodCall,
    ExprPath,
    Item,
    ItemUse,
    Lit,
    UseTree,
    visit::{Visit, visit_expr_method_call, visit_expr_path, visit_item, visit_item_use, visit_path},
};

/// The module that owns the Scope grammar, exempt from the guard.
const SCOPE_OWNER: &str = "crates/andromeda-ngsi-ld-lexicon/src/scope/";

/// The `scope` submodules whose import marks a source as one handling Scopes.
const SCOPE_MODULES: [&str; 5] = ["grammar", "level", "path", "pattern", "query"];

/// The string methods that split or strip text on a separator.
const SEPARATOR_METHODS: [&str; 8] = [
    "split",
    "rsplit",
    "split_once",
    "rsplit_once",
    "split_terminator",
    "strip_prefix",
    "strip_suffix",
    "trim_start_matches",
];

/// The level constructors that bypass `checked_level`.
const UNBOUNDED_LEVEL_TYPES: [&str; 2] = ["ScopeLevel", "ScopeLevelBuf"];

/// Every production site that reads the Scope grammar outside its owner, as
/// `path: what`, sorted.
#[must_use]
pub fn scope_grammar_violations(workspace_root: &str) -> Vec<String> {
    let mut violations = BTreeSet::new();
    for source in workspace_sources(Path::new(workspace_root)) {
        if source.workspace_path.starts_with(SCOPE_OWNER) || source.workspace_path.contains("/tests/") || source.workspace_path.ends_with("build.rs") {
            continue;
        }
        let mut visitor = ScopeGrammarVisitor::default();
        visitor.visit_file(&source.syntax);
        violations.extend(
            visitor
                .into_violations()
                .into_iter()
                .map(|violation| format!("{}: {violation}", source.workspace_path)),
        );
    }
    violations.into_iter().collect()
}

/// Assert that the Scope grammar is read only by its owner.
///
/// # Panics
///
/// Panics on any site [`scope_grammar_violations`] reports, or when the
/// workspace source tree cannot be read or parsed.
pub fn assert_scope_grammar_has_one_owner(workspace_root: &str) {
    let violations = scope_grammar_violations(workspace_root);
    assert!(
        violations.is_empty(),
        "the NGSI-LD Scope grammar is read outside {SCOPE_OWNER}; parse through `ScopePath`, split through `split_levels` and admit a level through `checked_level`:\n{}",
        violations.join("\n")
    );
}

/// Collects what one parsed source does with the Scope grammar.
#[derive(Default)]
struct ScopeGrammarVisitor {
    /// Whether the source imports a lexicon or query `scope` module.
    handles_scopes: bool,
    /// Violations that hold whatever the source imports.
    violations: Vec<String>,
    /// Separator splits, which are violations only in a source handling Scopes.
    separator_splits: Vec<String>,
}

impl ScopeGrammarVisitor {
    /// The violations this source commits.
    fn into_violations(self) -> Vec<String> {
        let mut violations = self.violations;
        if self.handles_scopes {
            violations.extend(self.separator_splits);
        }
        violations
    }
}

impl<'ast> Visit<'ast> for ScopeGrammarVisitor {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_is_test_only(item) {
            return;
        }
        visit_item(self, item);
    }

    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        let mut prefix = Vec::new();
        collect_use_paths(&item.tree, &mut prefix, &mut |segments| {
            if segments.windows(2).any(|pair| pair[0] == "scope" && SCOPE_MODULES.contains(&pair[1].as_str())) {
                self.handles_scopes = true;
            }
            if segments.windows(2).any(|pair| pair[0] == "scope" && pair[1] == "grammar") {
                self.violations.push("imports the raw `scope::grammar`".to_owned());
            }
        });
        visit_item_use(self, item);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        let segments: Vec<String> = path.segments.iter().map(|segment| segment.ident.to_string()).collect();
        if segments.windows(2).any(|pair| pair[0] == "scope" && pair[1] == "grammar") {
            self.violations.push("names the raw `scope::grammar`".to_owned());
        }
        visit_path(self, path);
    }

    fn visit_expr_path(&mut self, expression: &'ast ExprPath) {
        let segments: Vec<String> = expression.path.segments.iter().map(|segment| segment.ident.to_string()).collect();
        if let [.., owner, constructor] = segments.as_slice()
            && constructor == "new"
            && UNBOUNDED_LEVEL_TYPES.contains(&owner.as_str())
        {
            self.violations
                .push(format!("admits a level through `{owner}::new` instead of `checked_level`"));
        }
        visit_expr_path(self, expression);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        let method = call.method.to_string();
        if SEPARATOR_METHODS.contains(&method.as_str()) && call.args.iter().any(is_level_separator) {
            self.separator_splits.push(format!("calls `{method}` on the scope separator `/`"));
        }
        visit_expr_method_call(self, call);
    }
}

/// Call `found` with the full segment list of every path a `use` tree imports.
fn collect_use_paths(tree: &UseTree, prefix: &mut Vec<String>, found: &mut impl FnMut(&[String])) {
    match tree {
        UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            collect_use_paths(&path.tree, prefix, found);
            prefix.pop();
        }
        UseTree::Name(name) => {
            prefix.push(name.ident.to_string());
            found(prefix);
            prefix.pop();
        }
        UseTree::Rename(rename) => {
            prefix.push(rename.ident.to_string());
            found(prefix);
            prefix.pop();
        }
        UseTree::Glob(_) => found(prefix),
        UseTree::Group(group) => {
            for item in &group.items {
                collect_use_paths(item, prefix, found);
            }
        }
    }
}

/// Whether an argument is the `/` scope separator, as a `char` or a `&str`.
fn is_level_separator(argument: &Expr) -> bool {
    let Expr::Lit(ExprLit { lit, .. }) = argument else {
        return false;
    };
    // `syn::Lit` is non-exhaustive, so the two separator spellings are tested
    // for rather than every literal kind enumerated.
    matches!(lit, Lit::Char(character) if character.value() == '/') || matches!(lit, Lit::Str(text) if text.value() == "/")
}

#[cfg(test)]
mod tests {
    use crate::scope_grammar::ScopeGrammarVisitor;
    use syn::visit::Visit;

    fn violations(source: &str) -> Vec<String> {
        let syntax = syn::parse_file(source).unwrap_or_else(|error| panic!("fixture parses: {error}"));
        let mut visitor = ScopeGrammarVisitor::default();
        visitor.visit_file(&syntax);
        visitor.into_violations()
    }

    #[test]
    fn a_raw_level_constructor_is_flagged() {
        assert_eq!(violations("fn read(raw: &str) { let _ = ScopeLevel::new(raw); }").len(), 1);
        assert_eq!(violations("fn read(raw: &str) { let _ = level::ScopeLevelBuf::new(raw.to_owned()); }").len(), 1);
    }

    #[test]
    fn the_raw_grammar_type_is_flagged() {
        assert_ne!(violations("use andromeda_ngsi_ld_lexicon::scope::grammar::ScopeBuf;"), Vec::<String>::new());
        assert_ne!(
            violations("fn read(raw: &str) { let _ = andromeda_ngsi_ld_lexicon::scope::grammar::Scope::new(raw); }"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_scope_handling_source_that_splits_on_the_separator_is_flagged() {
        let source = "use andromeda_ngsi_ld_lexicon::scope::path::ScopePath; fn levels(raw: &str) -> usize { raw.split('/').count() }";
        assert_eq!(violations(source), vec!["calls `split` on the scope separator `/`".to_owned()]);
        let grouped = "use andromeda_ngsi_ld_query::{scope::{query::ScopeQuery}}; fn root(raw: &str) -> Option<&str> { raw.strip_prefix(\"/\") }";
        assert_eq!(violations(grouped).len(), 1);
    }

    #[test]
    fn a_source_that_handles_no_scope_may_split_on_a_slash() {
        assert_eq!(
            violations("use std::path::Path; fn segments(raw: &str) -> usize { raw.split('/').count() }"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn reading_through_the_owner_is_not_flagged() {
        let source = "use andromeda_ngsi_ld_lexicon::scope::{level::checked_level, path::{ScopePath, split_levels}}; \
                      fn read(raw: &str) -> usize { let _ = checked_level(raw); let _ = raw.parse::<ScopePath>(); split_levels(raw).count() }";
        assert_eq!(violations(source), Vec::<String>::new());
    }

    #[test]
    fn a_test_only_split_is_ignored() {
        let source = "use andromeda_ngsi_ld_lexicon::scope::path::ScopePath; #[cfg(test)] mod tests { fn f(raw: &str) -> usize { raw.split('/').count() } }";
        assert_eq!(violations(source), Vec::<String>::new());
    }
}
