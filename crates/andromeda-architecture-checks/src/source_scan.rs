//! Shared Rust-source discovery and parsing for source-based guards.

use std::{
    fs,
    path::{Path, PathBuf},
};
use syn::{Attribute, File, Item, UseTree};

/// A parsed Rust source file and its workspace-relative path.
pub(crate) struct RustSource {
    /// Absolute path used to resolve external modules.
    absolute_path: PathBuf,
    /// Parsed syntax tree.
    pub(crate) syntax: File,
    /// Workspace-relative path with forward slashes.
    pub(crate) workspace_path: String,
}

/// Read and parse every Rust source below the workspace production roots.
///
/// A file that cannot be read or does not parse leaves every invariant the
/// guards check unverified for that file, so the scan fails with each failing
/// path named rather than returning a silently smaller tree.
///
/// # Panics
///
/// Panics when a discovered source cannot be read or does not parse as Rust.
pub(crate) fn workspace_sources(workspace_root: &Path) -> Vec<RustSource> {
    let mut paths = ["src", "crates", "lib"]
        .into_iter()
        .flat_map(|root| rust_file_paths(&workspace_root.join(root)))
        .collect::<Vec<_>>();
    paths.sort();
    let mut unreadable = Vec::new();
    let mut sources = Vec::new();
    for path in &paths {
        if let Some(source) = parse_source(workspace_root, path, &mut unreadable) {
            sources.push(source);
        }
    }
    assert!(unreadable.is_empty(), "workspace sources could not be scanned:\n{}", unreadable.join("\n"));
    let excluded = sources.iter().flat_map(test_only_external_modules).collect::<Vec<_>>();
    sources.retain(|source| {
        !is_test_only(&source.syntax.attrs)
            && !excluded
                .iter()
                .any(|excluded_path| source.absolute_path == *excluded_path || source.absolute_path.starts_with(excluded_path))
    });
    sources
}

/// Read and parse every Rust source of the workspace, test code included.
///
/// Unlike [`workspace_sources`], nothing is filtered: integration tests,
/// test-only modules and build scripts are all returned, for a guard whose rule
/// holds for test code as much as for production code.
///
/// # Panics
///
/// Panics when a discovered source cannot be read or does not parse as Rust.
pub(crate) fn every_workspace_source(workspace_root: &Path) -> Vec<RustSource> {
    let mut paths = ["build.rs", "src", "tests", "crates", "lib"]
        .into_iter()
        .flat_map(|root| {
            let path = workspace_root.join(root);
            if path.is_file() { vec![path] } else { rust_file_paths(&path) }
        })
        .collect::<Vec<_>>();
    paths.sort();
    let mut unreadable = Vec::new();
    let sources = paths
        .iter()
        .filter_map(|path| parse_source(workspace_root, path, &mut unreadable))
        .collect::<Vec<_>>();
    assert!(unreadable.is_empty(), "workspace sources could not be scanned:\n{}", unreadable.join("\n"));
    sources
}

/// Recursively discover every Rust source below one directory, ordered by path.
///
/// A directory that does not exist holds no sources. One that exists but cannot
/// be listed fails the guard that asked for it: a scan that quietly skips a
/// subtree reads as enforcement while providing none.
///
/// # Panics
///
/// Panics when a directory below `root` cannot be listed.
pub(crate) fn rust_file_paths(root: &Path) -> Vec<PathBuf> {
    if !root.exists() {
        return Vec::new();
    }
    let mut paths = Vec::new();
    let mut unreadable = Vec::new();
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                unreadable.push(format!("{} could not be listed: {error}", directory.display()));
                continue;
            }
        };
        for entry in entries {
            let path = match entry {
                Ok(entry) => entry.path(),
                Err(error) => {
                    unreadable.push(format!("an entry of {} could not be read: {error}", directory.display()));
                    continue;
                }
            };
            if path.is_dir() {
                directories.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                paths.push(path);
            }
        }
    }
    assert!(
        unreadable.is_empty(),
        "the source tree below {} could not be listed:\n{}",
        root.display(),
        unreadable.join("\n")
    );
    paths.sort();
    paths
}

/// Whether an item is compiled only when the `test` configuration is active.
pub(crate) fn is_test_only(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("cfg")
            && quote::ToTokens::to_token_stream(&attribute.meta)
                .to_string()
                .split(|character: char| !character.is_alphanumeric() && character != '_')
                .any(|token| token == "test")
    })
}

/// Return from the enclosing function with the test-only answer for the first
/// listed item shape that matches.
///
/// Expands to one `if let` per named `syn::Item` variant, each returning
/// [`is_test_only`] over that variant's attributes. A `match` cannot express
/// this: `syn::Item` is `#[non_exhaustive]`, so it needs a catch-all arm whose
/// body would repeat every listed arm.
macro_rules! return_when_test_only {
    ($item:expr, $($shape:ident),+ $(,)?) => {
        $(
            if let Item::$shape(shape) = $item {
                return is_test_only(&shape.attrs);
            }
        )+
    };
}

/// Whether one inline item is compiled only when the `test` configuration is active.
///
/// An item shape not listed carries no attributes of its own, so nothing about
/// it can be test-only.
pub(crate) fn item_is_test_only(item: &Item) -> bool {
    return_when_test_only!(
        item,
        Const,
        Enum,
        ExternCrate,
        Fn,
        ForeignMod,
        Impl,
        Macro,
        Mod,
        Static,
        Struct,
        Trait,
        TraitAlias,
        Type,
        Union,
        Use,
    );
    false
}

/// Call `found` with the full segment list of every path a `use` tree imports.
pub(crate) fn collect_use_paths(tree: &UseTree, prefix: &mut Vec<String>, found: &mut impl FnMut(&[String])) {
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

/// Read and parse one source, retaining a stable workspace-relative identity.
///
/// A file that cannot be read or does not parse yields no source and one
/// message in `unreadable`, so the caller fails with every failing path named
/// rather than with the first.
fn parse_source(workspace_root: &Path, path: &Path, unreadable: &mut Vec<String>) -> Option<RustSource> {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            unreadable.push(format!("{} could not be read: {error}", path.display()));
            return None;
        }
    };
    let syntax = match syn::parse_file(&source) {
        Ok(syntax) => syntax,
        Err(error) => {
            unreadable.push(format!("{} does not parse as Rust: {error}", path.display()));
            return None;
        }
    };
    let workspace_path = path.strip_prefix(workspace_root).unwrap_or(path).to_string_lossy().replace('\\', "/");
    Some(RustSource {
        absolute_path: path.to_path_buf(),
        syntax,
        workspace_path,
    })
}

/// Resolve every external module declared behind a test-only configuration.
fn test_only_external_modules(source: &RustSource) -> Vec<PathBuf> {
    source
        .syntax
        .items
        .iter()
        .flat_map(|item| {
            let Item::Mod(module) = item else {
                return Vec::new();
            };
            if module.content.is_some() || !is_test_only(&module.attrs) {
                return Vec::new();
            }
            let parent = source.absolute_path.parent().unwrap_or(Path::new(""));
            let file_name = source.absolute_path.file_name().and_then(|name| name.to_str());
            let base = match file_name {
                Some("lib.rs" | "main.rs" | "mod.rs") | None => parent.to_path_buf(),
                Some(_) => parent.join(source.absolute_path.file_stem().unwrap_or_default()),
            };
            vec![base.join(format!("{}.rs", module.ident)), base.join(module.ident.to_string())]
        })
        .collect()
}
