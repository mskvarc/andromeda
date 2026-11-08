//! The untyped-tree names each module binds through its imports, so a child
//! module that reaches its parent's private import (`use crate::entity::Value`
//! where `entity` imports `serde_json::Value`) is read as naming the tree.

use crate::untyped_json::{forbidden_name::UntypedTree, source_reading::imported_trees};
use std::collections::BTreeMap;
use syn::File;

/// Where a source sits in its crate's module tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ModulePosition {
    /// The crate's `src` directory, workspace-relative.
    crate_root: String,
    /// The module path below the crate root (`entity::append_attributes`).
    module: Vec<String>,
}

impl ModulePosition {
    /// The position of the source at a workspace-relative path, when the path
    /// lies below a crate's `src` directory.
    pub(crate) fn of(workspace_path: &str) -> Option<ModulePosition> {
        let (crate_root, below) = if let Some(below) = workspace_path.strip_prefix("src/") {
            ("src".to_owned(), below)
        } else {
            let (crate_dir, below) = workspace_path.split_once("/src/")?;
            (format!("{crate_dir}/src"), below)
        };
        let below = below.strip_suffix(".rs")?;
        let mut module: Vec<String> = below.split('/').map(str::to_owned).collect();
        if module.last().is_some_and(|last| matches!(last.as_str(), "lib" | "main" | "mod")) {
            module.pop();
        }
        Some(ModulePosition { crate_root, module })
    }

    /// The module a crate-relative `use` path points into, and the name it
    /// imports from there; `None` for a path into another crate.
    fn target<'a>(&self, segments: &'a [String]) -> Option<(Vec<String>, &'a String)> {
        let (name, path) = segments.split_last()?;
        let (head, rest) = path.split_first()?;
        let module = match head.as_str() {
            "crate" => rest.to_vec(),
            "self" => self.module.iter().chain(rest).cloned().collect(),
            "super" => {
                let supers = 1 + rest.iter().take_while(|segment| *segment == "super").count();
                let kept = self.module.len().checked_sub(supers)?;
                self.module.iter().take(kept).chain(rest.iter().skip(supers - 1)).cloned().collect()
            }
            _ => return None,
        };
        Some((module, name))
    }
}

/// The untyped-tree names each module of the workspace binds, keyed by crate
/// root and module path.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ModuleImports(BTreeMap<(String, Vec<String>), BTreeMap<String, UntypedTree>>);

impl ModuleImports {
    /// Read the imports of every source until no module learns a new name: a
    /// module importing a name its parent imported from `serde_json` binds
    /// that tree too, and so on down the tree.
    pub(crate) fn settle(sources: &[(Option<ModulePosition>, &File)]) -> ModuleImports {
        let mut settled = ModuleImports::default();
        loop {
            let mut next = ModuleImports::default();
            for (position, syntax) in sources {
                let Some(position) = position else {
                    continue;
                };
                let names = imported_trees(syntax, &|segments| settled.resolve(Some(position), segments));
                if !names.is_empty() {
                    next.0.entry((position.crate_root.clone(), position.module.clone())).or_default().extend(names);
                }
            }
            if next == settled {
                return settled;
            }
            settled = next;
        }
    }

    /// The tree a crate-relative `use` path imports from a module that bound
    /// it, read from the position of the importing source.
    pub(crate) fn resolve(&self, position: Option<&ModulePosition>, segments: &[String]) -> Option<UntypedTree> {
        let position = position?;
        let (module, name) = position.target(segments)?;
        self.0.get(&(position.crate_root.clone(), module))?.get(name).copied()
    }
}
