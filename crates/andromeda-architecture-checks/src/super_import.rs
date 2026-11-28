//! The workspace-wide rule that an import names its path from the crate root.
//!
//! A `use super::...` means something else once its module moves one level
//! up or down; a `crate::` path names the item the same way from everywhere in
//! the crate. The rule holds for test modules as much as for production ones,
//! so the scan reads test code too. A `use` written inside a `macro_rules!`
//! body is not read: a declarative macro has no `crate::` spelling of the
//! module it is invoked in.

use crate::source_scan::every_workspace_source;
use std::path::Path;
use syn::{File, ItemUse, UseTree, visit::Visit};

/// Every `use` of the workspace whose path starts at `super`, as `path:line`.
#[must_use]
pub fn super_imports(workspace_root: &str) -> Vec<String> {
    let mut sites = every_workspace_source(Path::new(workspace_root))
        .iter()
        .flat_map(|source| {
            super_import_lines(&source.syntax)
                .into_iter()
                .map(|line| format!("{}:{line}", source.workspace_path))
        })
        .collect::<Vec<_>>();
    sites.sort();
    sites
}

/// Assert that no `use` of the workspace starts its path at `super`.
///
/// # Panics
///
/// Panics naming every such import, or when the source tree cannot be read.
pub fn assert_no_super_imports(workspace_root: &str) {
    let sites = super_imports(workspace_root);
    assert!(
        sites.is_empty(),
        "imports that start at super; import from the crate root with crate:: instead:\n{}",
        sites.join("\n")
    );
}

/// The lines of `syntax` holding a `use` that starts at `super`.
fn super_import_lines(syntax: &File) -> Vec<usize> {
    let mut finder = SuperImportFinder::default();
    finder.visit_file(syntax);
    finder.lines
}

/// Collects the `use` items whose tree starts at `super`.
#[derive(Default)]
struct SuperImportFinder {
    /// The line of each such item, in source order.
    lines: Vec<usize>,
}

impl<'ast> Visit<'ast> for SuperImportFinder {
    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        if starts_at_super(&item.tree) {
            self.lines.push(item.use_token.span.start().line);
        }
    }
}

/// Whether a `use` tree starts at `super`, directly or inside a leading group.
fn starts_at_super(tree: &UseTree) -> bool {
    match tree {
        UseTree::Path(path) => path.ident == "super",
        UseTree::Name(name) => name.ident == "super",
        UseTree::Rename(rename) => rename.ident == "super",
        UseTree::Group(group) => group.items.iter().any(starts_at_super),
        UseTree::Glob(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::super_import::super_import_lines;
    use pretty_assertions::assert_eq;

    fn lines(source: &str) -> Vec<usize> {
        super_import_lines(&syn::parse_file(source).unwrap_or_else(|error| panic!("fixture parses: {error}")))
    }

    #[test]
    fn a_glob_and_a_named_import_from_super_are_found_in_a_test_module() {
        assert_eq!(
            lines("fn f() {}\n#[cfg(test)]\nmod tests {\n    use super::*;\n    use super::{f, g};\n}\n"),
            vec![4, 5]
        );
    }

    #[test]
    fn a_super_path_inside_a_leading_group_is_found() {
        assert_eq!(lines("use {super::f, std::fmt};\n"), vec![1]);
    }

    #[test]
    fn crate_and_external_imports_are_not_found() {
        assert_eq!(lines("use crate::a::b;\nuse std::fmt;\nuse self::c::d;\n"), Vec::<usize>::new());
    }

    #[test]
    fn a_use_inside_a_macro_rules_body_is_not_read() {
        assert_eq!(lines("macro_rules! m {\n    () => { mod x { use super::y; } };\n}\n"), Vec::<usize>::new());
    }
}
