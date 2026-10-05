//! The walk that finds every untyped JSON name in one parsed source.
//!
//! Names are found in two passes. The `use` declarations are read from the
//! syntax tree, so every imported leaf is one finding and its local name (or
//! rename) is remembered. Everything else is read as tokens, because a tree
//! named inside a macro call (`vec![json!(..)]`, `assert!(matches!(v,
//! Value::Null))`) is invisible to a syntax visitor: a qualified path counts
//! once at its first segment, and a remembered local name counts wherever it
//! stands on its own (not as a field or method after `.`, not as a later
//! segment of another path).

use crate::{
    source_scan::{is_test_only, item_is_test_only},
    untyped_json::forbidden_name::{UntypedTree, names_the_json_macro, untyped_tree_of},
};
use proc_macro2::{Spacing, TokenStream, TokenTree};
use quote::ToTokens;
use std::{collections::BTreeMap, iter::once};
use syn::{
    File,
    ImplItem,
    Item,
    ItemUse,
    UseTree,
    spanned::Spanned,
    visit::{Visit, visit_impl_item, visit_item},
};

/// One untyped JSON name found in a source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Finding {
    /// One-based line the name starts on.
    pub(crate) line: usize,
    /// The name as written, path segments joined by `::`.
    pub(crate) subject: String,
    /// The tree the name belongs to.
    pub(crate) tree: UntypedTree,
}

/// What a local name imported from a forbidden path stands for.
#[derive(Clone, Copy, Debug)]
struct ImportedName {
    /// The tree the import names.
    tree: UntypedTree,
    /// Whether the name is the `json!` macro, counted only as a macro call.
    macro_call: bool,
}

/// Resolves a crate-relative `use` path to the tree the module it points into
/// imported under that name.
pub(crate) type InheritedImport<'r> = &'r dyn Fn(&[String]) -> Option<UntypedTree>;

/// The local names a source's production imports bind to an untyped tree.
pub(crate) fn imported_trees(syntax: &File, inherited: InheritedImport<'_>) -> BTreeMap<String, UntypedTree> {
    let mut imports = ImportReader::new(inherited);
    imports.visit_file(syntax);
    imports.imported.into_iter().map(|(name, imported)| (name, imported.tree)).collect()
}

/// Every untyped JSON name in the production items of one parsed source, in
/// source order.
pub(crate) fn untyped_json_findings(syntax: &File, inherited: InheritedImport<'_>) -> Vec<Finding> {
    let mut imports = ImportReader::new(inherited);
    imports.visit_file(syntax);
    let mut scan = TokenScan {
        imported: imports.imported,
        findings: imports.findings,
    };
    syntax.items.iter().for_each(|item| scan.item(item));
    let mut findings = scan.findings;
    findings.sort_by_key(|finding| finding.line);
    findings
}

/// Reads the production `use` declarations of a source, at every depth.
struct ImportReader<'r> {
    /// The trees the modules this source imports from bound.
    inherited: InheritedImport<'r>,
    /// Local names bound to a forbidden path.
    imported: BTreeMap<String, ImportedName>,
    /// One finding per imported forbidden leaf.
    findings: Vec<Finding>,
}

impl<'r> ImportReader<'r> {
    /// A reader that has seen no import yet.
    fn new(inherited: InheritedImport<'r>) -> ImportReader<'r> {
        ImportReader {
            inherited,
            imported: BTreeMap::new(),
            findings: Vec::new(),
        }
    }

    /// Record every leaf of a `use` tree below `prefix`.
    fn tree(&mut self, tree: &UseTree, prefix: &mut Vec<String>) {
        match tree {
            UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                self.tree(&path.tree, prefix);
                prefix.pop();
            }
            // clone: the bound name outlives the walk's prefix, which the siblings still extend
            UseTree::Name(name) if name.ident == "self" => self.leaf(prefix, prefix.last().cloned(), name.span().start().line),
            UseTree::Name(name) => self.leaf(
                &extended(prefix, name.ident.to_string()),
                Some(name.ident.to_string()),
                name.span().start().line,
            ),
            UseTree::Rename(rename) => {
                let leaf = extended(prefix, rename.ident.to_string());
                self.leaf(&leaf, Some(rename.rename.to_string()), rename.span().start().line);
            }
            UseTree::Glob(glob) => self.leaf(&extended(prefix, "*".to_owned()), None, glob.span().start().line),
            UseTree::Group(group) => group.items.iter().for_each(|item| self.tree(item, prefix)),
        }
    }

    /// Record one imported leaf when it names an untyped tree.
    fn leaf(&mut self, segments: &[String], bound: Option<String>, line: usize) {
        let Some(tree) = untyped_tree_of(segments).or_else(|| (self.inherited)(segments)) else {
            return;
        };
        if let Some(bound) = bound {
            let macro_call = names_the_json_macro(segments);
            self.imported.insert(bound, ImportedName { tree, macro_call });
        }
        self.findings.push(Finding {
            line,
            subject: segments.join("::"),
            tree,
        });
    }
}

impl<'ast> Visit<'ast> for ImportReader<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_is_test_only(item) {
            return;
        }
        visit_item(self, item);
    }

    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        if impl_item_is_test_only(item) {
            return;
        }
        visit_impl_item(self, item);
    }

    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        self.tree(&item.tree, &mut Vec::new());
    }
}

/// Reads the tokens of every production item that is not a `use` declaration.
struct TokenScan {
    /// Local names bound to a forbidden path by the source's imports.
    imported: BTreeMap<String, ImportedName>,
    /// Findings so far, starting with the imports.
    findings: Vec<Finding>,
}

impl TokenScan {
    /// Scan one item, descending into inline modules and impl blocks so their
    /// test-only members are skipped.
    fn item(&mut self, item: &Item) {
        if item_is_test_only(item) || matches!(item, Item::Use(_)) {
            return;
        }
        if let Item::Mod(module) = item {
            for item in module.content.iter().flat_map(|(_, items)| items) {
                self.item(item);
            }
            return;
        }
        if let Item::Impl(block) = item {
            self.tokens(block.self_ty.to_token_stream());
            if let Some((path, _)) = &block.trait_ {
                self.tokens(path.to_token_stream());
            }
            self.tokens(block.generics.to_token_stream());
            if let Some(clause) = &block.generics.where_clause {
                self.tokens(clause.to_token_stream());
            }
            for member in block.items.iter().filter(|member| !impl_item_is_test_only(member)) {
                self.tokens(member.to_token_stream());
            }
            return;
        }
        self.tokens(item.to_token_stream());
    }

    /// Scan one token stream, one nesting level at a time.
    fn tokens(&mut self, stream: TokenStream) {
        let tokens: Vec<TokenTree> = stream.into_iter().collect();
        let mut index = 0;
        while let Some(token) = tokens.get(index) {
            index = match token {
                TokenTree::Group(group) => {
                    self.tokens(group.stream());
                    index + 1
                }
                TokenTree::Ident(ident) if ident == "use" => statement_end(&tokens, index),
                TokenTree::Ident(_) => self.name(&tokens, index),
                TokenTree::Punct(_) | TokenTree::Literal(_) => index + 1,
            };
        }
    }

    /// Record the name starting at `index` if it names an untyped tree, and
    /// return the index after it.
    fn name(&mut self, tokens: &[TokenTree], index: usize) -> usize {
        if follows_path_separator(tokens, index) || follows_dot(tokens, index) {
            return index + 1;
        }
        let (segments, next) = path_at(tokens, index);
        let line = tokens.get(index).map_or(0, |token| token.span().start().line);
        if let Some(tree) = untyped_tree_of(&segments) {
            self.findings.push(Finding {
                line,
                subject: segments.join("::"),
                tree,
            });
            return next;
        }
        let Some(head) = segments.first() else {
            return next;
        };
        if let Some(imported) = self.imported.get(head).copied() {
            let macro_call = matches!(tokens.get(index + 1), Some(TokenTree::Punct(punct)) if punct.as_char() == '!');
            if !imported.macro_call || macro_call {
                self.findings.push(Finding {
                    line,
                    subject: segments.join("::"),
                    tree: imported.tree,
                });
            }
        }
        next
    }
}

/// Whether an impl member is compiled only when the `test` configuration is
/// active.
///
/// `syn::ImplItem` is `#[non_exhaustive]`, so the shapes that carry attributes
/// are tested one by one; any other shape carries none and is never test-only.
fn impl_item_is_test_only(item: &ImplItem) -> bool {
    if let ImplItem::Const(member) = item {
        return is_test_only(&member.attrs);
    }
    if let ImplItem::Fn(member) = item {
        return is_test_only(&member.attrs);
    }
    if let ImplItem::Type(member) = item {
        return is_test_only(&member.attrs);
    }
    if let ImplItem::Macro(member) = item {
        return is_test_only(&member.attrs);
    }
    false
}

/// The path starting at `index` (`a::b::c`), and the index after it.
fn path_at(tokens: &[TokenTree], index: usize) -> (Vec<String>, usize) {
    let mut segments = Vec::new();
    let mut position = index;
    while let Some(TokenTree::Ident(ident)) = tokens.get(position) {
        segments.push(ident.to_string());
        if !is_path_separator(tokens, position + 1) {
            return (segments, position + 1);
        }
        position += 3;
    }
    (segments, position)
}

/// Whether `::` starts at `index`.
fn is_path_separator(tokens: &[TokenTree], index: usize) -> bool {
    matches!(
        (tokens.get(index), tokens.get(index + 1)),
        (Some(TokenTree::Punct(first)), Some(TokenTree::Punct(second)))
            if first.as_char() == ':' && first.spacing() == Spacing::Joint && second.as_char() == ':'
    )
}

/// Whether the token at `index` continues a path (`x::name`).
fn follows_path_separator(tokens: &[TokenTree], index: usize) -> bool {
    index >= 2 && is_path_separator(tokens, index - 2)
}

/// Whether the token at `index` is a field or method after `.`.
fn follows_dot(tokens: &[TokenTree], index: usize) -> bool {
    index >= 1 && matches!(tokens.get(index - 1), Some(TokenTree::Punct(punct)) if punct.as_char() == '.')
}

/// The index after the `;` that ends the statement starting at `index`.
///
/// A nested `use` declaration was already read by the import pass, so its
/// tokens are skipped here rather than counted twice.
fn statement_end(tokens: &[TokenTree], index: usize) -> usize {
    tokens
        .iter()
        .enumerate()
        .skip(index)
        .find(|(_, token)| matches!(token, TokenTree::Punct(punct) if punct.as_char() == ';'))
        .map_or(tokens.len(), |(position, _)| position + 1)
}

/// `prefix` followed by one more segment.
///
/// The prefix is cloned because the walk keeps it for the leaf's siblings,
/// while the finding owns the full path.
fn extended(prefix: &[String], segment: String) -> Vec<String> {
    prefix.iter().cloned().chain(once(segment)).collect()
}
