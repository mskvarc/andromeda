//! The syntax walk that finds, in one parsed source, each pattern naming a
//! variant of a `q` term's syntax tree.

use crate::{
    q_reading::imports::{QUERY_MODULE, TermImports},
    source_scan::item_is_test_only,
};
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use std::mem;
use syn::{
    File,
    Item,
    ItemImpl,
    Macro,
    Pat,
    PatIdent,
    Type,
    visit::{Visit, visit_item, visit_item_impl, visit_macro, visit_pat, visit_pat_ident, visit_path},
};

/// Collects the patterns of one parsed source that name a term variant.
pub(crate) struct QReadingVisitor {
    /// The imports of the source that bring a watched type or variant into
    /// scope under a name of its own.
    imports: TermImports,
    /// The watched type of the `impl` block being visited, which `Self` names.
    impl_owner: Option<String>,
    /// How many patterns enclose the current node.
    pattern_depth: usize,
    /// Each site found, as its line and what it names.
    sites: Vec<(usize, String)>,
}

impl QReadingVisitor {
    /// A visitor for `file`, holding the sites its glob imports are.
    pub(crate) fn for_file(file: &File) -> Self {
        let imports = TermImports::collect(file);
        let sites = imports
            .glob_imports()
            .iter()
            .map(|(line, owner)| (*line, format!("imports every variant of `{owner}`")))
            .collect();
        Self {
            imports,
            impl_owner: None,
            pattern_depth: 0,
            sites,
        }
    }

    /// Every site found, in the order of the source.
    pub(crate) fn into_sites(mut self) -> Vec<(usize, String)> {
        self.sites.sort();
        self.sites
    }

    /// Record a pattern naming `owner::variant` at `line`.
    fn flag(&mut self, line: usize, owner: &str, variant: &str) {
        self.sites.push((line, format!("matches on `{owner}::{variant}`")));
    }

    /// The watched type a path segment `name` stands for: the type itself,
    /// a rename of it, or `Self` inside an `impl` of it. `qualified` says the
    /// path already led through the query crate's module.
    fn owner_named(&self, name: &str, qualified: bool) -> Option<String> {
        if name == "Self" {
            return self.impl_owner.clone();
        }
        let owner = if qualified { TermImports::qualified_owner(name) } else { None };
        owner.or_else(|| self.imports.owner(name)).map(str::to_owned)
    }

    /// Flag every variant the pattern tokens of a `matches!` name, qualified
    /// or imported by name: the tokens after its first top-level comma, at any
    /// depth of grouping.
    fn check_matches_tokens(&mut self, tokens: TokenStream) {
        let mut pattern = Vec::new();
        flatten(
            tokens
                .into_iter()
                .skip_while(|token| !matches!(token, TokenTree::Punct(punct) if punct.as_char() == ',')),
            &mut pattern,
        );
        for window in pattern.windows(4) {
            if let [
                TokenTree::Ident(owner),
                TokenTree::Punct(first),
                TokenTree::Punct(second),
                TokenTree::Ident(variant),
            ] = window
                && first.as_char() == ':'
                && second.as_char() == ':'
                && let Some(owner_type) = self.owner_named(&owner.to_string(), false)
            {
                self.flag(owner.span().start().line, &owner_type, &variant.to_string());
            }
        }
        for (index, token) in pattern.iter().enumerate() {
            let TokenTree::Ident(name) = token else { continue };
            let after_path = index > 0 && matches!(&pattern[index - 1], TokenTree::Punct(punct) if punct.as_char() == ':');
            let before_path = matches!(pattern.get(index + 1), Some(TokenTree::Punct(punct)) if punct.as_char() == ':');
            if !after_path
                && !before_path
                && let Some(owner) = self.imports.variant_owner(&name.to_string())
            {
                let owner = owner.to_owned();
                self.flag(name.span().start().line, &owner, &name.to_string());
            }
        }
    }

    /// Find every `matches!` invocation in the tokens of another macro, at
    /// any depth of grouping.
    fn check_nested_macros(&mut self, tokens: TokenStream) {
        let tokens: Vec<TokenTree> = tokens.into_iter().collect();
        for (index, token) in tokens.iter().enumerate() {
            if let TokenTree::Group(group) = token {
                let invoked_matches = index >= 2
                    && matches!(&tokens[index - 2], TokenTree::Ident(name) if name == "matches")
                    && matches!(&tokens[index - 1], TokenTree::Punct(bang) if bang.as_char() == '!')
                    && group.delimiter() != Delimiter::None;
                if invoked_matches {
                    self.check_matches_tokens(group.stream());
                } else {
                    self.check_nested_macros(group.stream());
                }
            }
        }
    }
}

/// Append `tokens` to `flat`, each group replaced by the tokens it holds.
fn flatten(tokens: impl IntoIterator<Item = TokenTree>, flat: &mut Vec<TokenTree>) {
    for token in tokens {
        if let TokenTree::Group(group) = token {
            flatten(group.stream(), flat);
        } else {
            flat.push(token);
        }
    }
}

impl<'ast> Visit<'ast> for QReadingVisitor {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_is_test_only(item) {
            return;
        }
        visit_item(self, item);
    }

    fn visit_item_impl(&mut self, item: &'ast ItemImpl) {
        let owner = if let Type::Path(path) = &*item.self_ty {
            let qualified = path.path.segments.iter().any(|segment| segment.ident == QUERY_MODULE);
            path.path
                .segments
                .last()
                .and_then(|segment| self.owner_named(&segment.ident.to_string(), qualified))
        } else {
            None
        };
        let outer = mem::replace(&mut self.impl_owner, owner);
        visit_item_impl(self, item);
        self.impl_owner = outer;
    }

    fn visit_pat_ident(&mut self, pattern: &'ast PatIdent) {
        let name = pattern.ident.to_string();
        if let Some(owner) = self.imports.variant_owner(&name) {
            let owner = owner.to_owned();
            self.flag(pattern.ident.span().start().line, &owner, &name);
        }
        visit_pat_ident(self, pattern);
    }

    fn visit_pat(&mut self, pattern: &'ast Pat) {
        self.pattern_depth += 1;
        visit_pat(self, pattern);
        self.pattern_depth -= 1;
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        if self.pattern_depth > 0 {
            let segments: Vec<&syn::PathSegment> = path.segments.iter().collect();
            let qualified = segments.iter().any(|segment| segment.ident == QUERY_MODULE);
            for pair in segments.windows(2) {
                if let Some(owner) = self.owner_named(&pair[0].ident.to_string(), qualified) {
                    self.flag(pair[0].ident.span().start().line, &owner, &pair[1].ident.to_string());
                }
            }
            if let [only] = segments.as_slice()
                && let Some(owner) = self.imports.variant_owner(&only.ident.to_string())
            {
                let owner = owner.to_owned();
                self.flag(only.ident.span().start().line, &owner, &only.ident.to_string());
            }
        }
        visit_path(self, path);
    }

    fn visit_macro(&mut self, invocation: &'ast Macro) {
        // A macro's arguments are tokens the parser does not build patterns
        // from, so a `matches!` pattern is read from its tokens.
        if invocation.path.is_ident("matches") {
            self.check_matches_tokens(invocation.tokens.clone());
        } else {
            self.check_nested_macros(invocation.tokens.clone());
        }
        visit_macro(self, invocation);
    }
}
