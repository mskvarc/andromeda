//! Shared Rust-source discovery and parsing for source-based guards.

use std::{
    fs,
    path::{Path, PathBuf},
};
use syn::{Attribute, File};

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
pub(crate) fn workspace_sources(workspace_root: &Path) -> Vec<RustSource> {
    let roots = ["src", "crates", "lib"];
    let mut paths = roots
        .iter()
        .map(|root| workspace_root.join(root))
        .filter(|root| root.exists())
        .flat_map(rust_source_paths)
        .collect::<Vec<_>>();
    paths.sort();
    let mut sources = paths.into_iter().map(|path| parse_source(workspace_root, &path)).collect::<Vec<_>>();
    let excluded = sources.iter().flat_map(test_only_external_modules).collect::<Vec<_>>();
    sources.retain(|source| {
        !is_test_only(&source.syntax.attrs)
            && !excluded
                .iter()
                .any(|excluded_path| source.absolute_path == *excluded_path || source.absolute_path.starts_with(excluded_path))
    });
    sources
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

/// Recursively discover Rust sources below one root.
fn rust_source_paths(root: PathBuf) -> Vec<PathBuf> {
    let mut sources = Vec::new();
    let mut directories = vec![root];
    while let Some(directory) = directories.pop() {
        let entries = fs::read_dir(&directory).unwrap_or_else(|error| unreachable!("{} is readable: {error}", directory.display()));
        for entry in entries {
            let path = entry
                .unwrap_or_else(|error| unreachable!("an entry of {} is readable: {error}", directory.display()))
                .path();
            if path.is_dir() {
                directories.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                sources.push(path);
            }
        }
    }
    sources
}

/// Read and parse one source, retaining a stable workspace-relative identity.
fn parse_source(workspace_root: &Path, path: &Path) -> RustSource {
    let source = fs::read_to_string(path).unwrap_or_else(|error| unreachable!("{} is readable: {error}", path.display()));
    let syntax = syn::parse_file(&source).unwrap_or_else(|error| unreachable!("{} parses as Rust: {error}", path.display()));
    let workspace_path = path.strip_prefix(workspace_root).unwrap_or(path).to_string_lossy().replace('\\', "/");
    RustSource {
        absolute_path: path.to_path_buf(),
        syntax,
        workspace_path,
    }
}

/// Resolve every external module declared behind a test-only configuration.
fn test_only_external_modules(source: &RustSource) -> Vec<PathBuf> {
    source
        .syntax
        .items
        .iter()
        .flat_map(|item| {
            let syn::Item::Mod(module) = item else {
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
