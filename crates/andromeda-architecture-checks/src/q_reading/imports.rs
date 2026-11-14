//! The `use` declarations of one parsed source that bring a `q` term type, or
//! one of its variants, into scope under a name of its own.

use crate::source_scan::item_is_test_only;
use std::collections::{BTreeMap, BTreeSet};
use syn::{
    File,
    Item,
    ItemUse,
    UseTree,
    visit::{Visit, visit_item},
};

/// The types whose variants a reading matches on: the syntax tree of a `q`
/// term and the scalar, operator and polarity types the datatype-mismatch
/// rules of clause 4.9 are decided on.
pub(super) const TERM_TYPES: [&str; 7] = ["Predicate", "SetMembership", "OrderingOp", "CompareValue", "Scalar", "CmpOp", "PatternPolarity"];

/// The watched types another crate of the workspace defines under the same
/// name, so a source reads the query crate's type only when it imports it from
/// the `filter` module or spells that path out.
const SHARED_NAMES: [&str; 1] = ["Scalar"];

/// The path segment that leads to the query crate's types.
pub(super) const QUERY_MODULE: &str = "filter";

/// How a source names a watched type and its variants without writing
/// `Type::Variant`.
#[derive(Default)]
pub(super) struct TermImports {
    /// A watched type renamed by `use Type as Alias`, as alias and type.
    aliases: BTreeMap<String, String>,
    /// The names in [`SHARED_NAMES`] the source imports from the query crate.
    query_owned: BTreeSet<String>,
    /// A variant imported by name, as the name in scope and its type.
    variants: BTreeMap<String, String>,
    /// Each glob import of a watched type's variants, as its line and the type.
    globs: Vec<(usize, String)>,
}

impl TermImports {
    /// The imports of every production item of `file`.
    pub(super) fn collect(file: &File) -> Self {
        let mut imports = Self::default();
        imports.visit_file(file);
        imports
    }

    /// The watched type `name` stands for, when it is one or renames one.
    pub(super) fn owner<'a>(&'a self, name: &'a str) -> Option<&'a str> {
        let watched = TERM_TYPES.contains(&name) && (!SHARED_NAMES.contains(&name) || self.query_owned.contains(name));
        watched.then_some(name).or_else(|| self.aliases.get(name).map(String::as_str))
    }

    /// The watched type `name` stands for when the path it was written in
    /// already leads through the query crate.
    pub(super) fn qualified_owner(name: &str) -> Option<&str> {
        TERM_TYPES.contains(&name).then_some(name)
    }

    /// The watched type a bare `name` is a variant of, when an import put it
    /// in scope by name.
    pub(super) fn variant_owner(&self, name: &str) -> Option<&str> {
        self.variants.get(name).map(String::as_str)
    }

    /// Each glob import of a watched type's variants, as its line and the type.
    pub(super) fn glob_imports(&self) -> &[(usize, String)] {
        &self.globs
    }

    /// Record what one use tree imports, `prefix` being the path above it.
    fn record(&mut self, tree: &UseTree, prefix: &mut Vec<String>) {
        match tree {
            UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                self.record(&path.tree, prefix);
                prefix.pop();
            }
            UseTree::Name(name) => self.record_leaf(prefix, &name.ident.to_string(), &name.ident.to_string()),
            UseTree::Rename(rename) => self.record_leaf(prefix, &rename.ident.to_string(), &rename.rename.to_string()),
            UseTree::Glob(glob) => {
                if let Some(owner) = prefix.last().and_then(|last| self.owner(last)) {
                    let owner = owner.to_owned();
                    self.globs.push((glob.star_token.spans[0].start().line, owner));
                }
            }
            UseTree::Group(group) => {
                for item in &group.items {
                    self.record(item, prefix);
                }
            }
        }
    }

    /// Record the leaf `imported` of a path, in scope as `local`.
    fn record_leaf(&mut self, prefix: &[String], imported: &str, local: &str) {
        if let Some(owner) = prefix.last().and_then(|last| self.owner(last)) {
            if imported != "self" {
                let owner = owner.to_owned();
                self.variants.insert(local.to_owned(), owner);
            }
        } else if TERM_TYPES.contains(&imported) {
            if SHARED_NAMES.contains(&imported) && prefix.iter().any(|segment| segment == QUERY_MODULE) {
                self.query_owned.insert(local.to_owned());
            }
            if imported != local {
                self.aliases.insert(local.to_owned(), imported.to_owned());
            }
        }
    }
}

impl<'ast> Visit<'ast> for TermImports {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_is_test_only(item) {
            return;
        }
        visit_item(self, item);
    }

    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        self.record(&item.tree, &mut Vec::new());
    }
}
