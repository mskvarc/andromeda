//! The syntax walk that finds, in one parsed source, each pattern naming a
//! variant of a `q` term's syntax tree.

use crate::source_scan::item_is_test_only;
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::{
    Item,
    Macro,
    Pat,
    visit::{Visit, visit_item, visit_macro, visit_pat, visit_path},
};

/// The syntax tree types whose variants a reading matches on.
const TERM_TYPES: [&str; 4] = ["Predicate", "SetMembership", "OrderingOp", "CompareValue"];

/// Collects the patterns of one parsed source that name a term variant.
#[derive(Default)]
pub(crate) struct QReadingVisitor {
    /// How many patterns enclose the current node.
    pattern_depth: usize,
    /// Each site found, as its line and what it names.
    sites: Vec<(usize, String)>,
}

impl QReadingVisitor {
    /// Every site found, in the order of the source.
    pub(crate) fn into_sites(mut self) -> Vec<(usize, String)> {
        self.sites.sort();
        self.sites
    }

    /// Record a pattern naming `owner::variant` at `line`.
    fn flag(&mut self, line: usize, owner: &str, variant: &str) {
        self.sites.push((line, format!("matches on `{owner}::{variant}`")));
    }

    /// Flag every `owner::variant` the pattern tokens of a `matches!` name:
    /// the tokens after its first top-level comma, at any depth of grouping.
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
                && TERM_TYPES.contains(&owner.to_string().as_str())
            {
                self.flag(owner.span().start().line, &owner.to_string(), &variant.to_string());
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

    fn visit_pat(&mut self, pattern: &'ast Pat) {
        self.pattern_depth += 1;
        visit_pat(self, pattern);
        self.pattern_depth -= 1;
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        if self.pattern_depth > 0 {
            let segments: Vec<&syn::PathSegment> = path.segments.iter().collect();
            for pair in segments.windows(2) {
                let owner = pair[0].ident.to_string();
                if TERM_TYPES.contains(&owner.as_str()) {
                    self.flag(pair[0].ident.span().start().line, &owner, &pair[1].ident.to_string());
                }
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
