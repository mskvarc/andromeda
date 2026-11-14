//! The snapshot names the sources of one crate take, read from the arguments
//! of the snapshot assertion macros.

use crate::orphan_snapshot::name_pattern::NamePattern;
use proc_macro2::{TokenStream, TokenTree};
use std::collections::BTreeMap;
use syn::{
    Expr,
    File,
    Lit,
    LitStr,
    Local,
    Macro,
    Pat,
    visit::{Visit, visit_local, visit_macro},
};

/// The macros whose first argument names a snapshot.
const ASSERTION_MACROS: [&str; 2] = ["assert_statement_snapshot", "assert_named_snapshot"];

/// What the sources of one crate say about the snapshots they take.
#[derive(Default)]
pub(super) struct References {
    /// Every name the sources can take a snapshot under.
    pub(super) patterns: Vec<NamePattern>,
    /// The assertions whose name is no literal, no `format!` and no binding
    /// of either, as `path:line`.
    pub(super) unresolved: Vec<String>,
}

/// The references one parsed source holds, added to `into`.
pub(super) fn collect_references(path: &str, syntax: &File, into: &mut References) {
    let mut visitor = ReferenceVisitor::default();
    visitor.visit_file(syntax);
    for (line, name) in visitor.invocations {
        match resolve(&name, &visitor.bindings) {
            Some(patterns) => into.patterns.extend(patterns),
            None => into.unresolved.push(format!("{path}:{line}")),
        }
    }
}

/// Walks one source for assertion invocations and for the `let` bindings a
/// name can be passed through.
#[derive(Default)]
struct ReferenceVisitor {
    /// Each invocation as its line and the tokens of its name argument.
    invocations: Vec<(usize, Vec<TokenTree>)>,
    /// The patterns each local name is bound to by a `let`.
    bindings: BTreeMap<String, Vec<NamePattern>>,
}

impl ReferenceVisitor {
    /// Record every assertion invocation among `tokens`, at any depth of
    /// grouping.
    fn scan(&mut self, tokens: TokenStream) {
        let tokens: Vec<TokenTree> = tokens.into_iter().collect();
        for (index, token) in tokens.iter().enumerate() {
            let TokenTree::Group(group) = token else { continue };
            let invoked = index >= 2
                && matches!(&tokens[index - 1], TokenTree::Punct(bang) if bang.as_char() == '!')
                && matches!(&tokens[index - 2], TokenTree::Ident(name) if ASSERTION_MACROS.iter().any(|macro_name| name == macro_name));
            if invoked {
                self.record(tokens[index - 2].span().start().line, group.stream());
            } else {
                self.scan(group.stream());
            }
        }
    }

    /// Record an invocation whose arguments are `arguments`.
    fn record(&mut self, line: usize, arguments: TokenStream) {
        let name = arguments
            .into_iter()
            .take_while(|token| !matches!(token, TokenTree::Punct(punct) if punct.as_char() == ','))
            .collect();
        self.invocations.push((line, name));
    }
}

impl<'ast> Visit<'ast> for ReferenceVisitor {
    fn visit_local(&mut self, local: &'ast Local) {
        if let Pat::Ident(binding) = &local.pat
            && let Some(init) = &local.init
            && let Some(patterns) = expression_patterns(&init.expr)
        {
            self.bindings.entry(binding.ident.to_string()).or_default().extend(patterns);
        }
        visit_local(self, local);
    }

    fn visit_macro(&mut self, invocation: &'ast Macro) {
        let last = invocation.path.segments.last().map(|segment| &segment.ident);
        match last {
            Some(name) if name == "macro_rules" => {}
            Some(name) if ASSERTION_MACROS.iter().any(|macro_name| name == macro_name) => {
                self.record(name.span().start().line, invocation.tokens.clone());
            }
            Some(_) | None => self.scan(invocation.tokens.clone()),
        }
        visit_macro(self, invocation);
    }
}

/// The patterns a name expression stands for, when it is a string literal or
/// a `format!` over one.
fn expression_patterns(expression: &Expr) -> Option<Vec<NamePattern>> {
    if let Expr::Lit(literal) = expression
        && let Lit::Str(text) = &literal.lit
    {
        return Some(vec![NamePattern::parse(&text.value())]);
    }
    if let Expr::Macro(invocation) = expression
        && invocation.mac.path.is_ident("format")
    {
        return format_pattern(invocation.mac.tokens.clone()).map(|pattern| vec![pattern]);
    }
    None
}

/// The pattern of the format string a `format!` opens with.
fn format_pattern(arguments: TokenStream) -> Option<NamePattern> {
    let first = arguments.into_iter().next()?;
    let text: LitStr = syn::parse2(TokenStream::from(first)).ok()?;
    Some(NamePattern::parse(&text.value()))
}

/// The patterns a name argument stands for: a string literal, a `format!`, or
/// a local name `let` bound to either.
fn resolve(name: &[TokenTree], bindings: &BTreeMap<String, Vec<NamePattern>>) -> Option<Vec<NamePattern>> {
    match name {
        [literal @ TokenTree::Literal(_)] => {
            let text: LitStr = syn::parse2(TokenStream::from(literal.clone())).ok()?;
            Some(vec![NamePattern::parse(&text.value())])
        }
        [TokenTree::Ident(function), TokenTree::Punct(bang), TokenTree::Group(arguments)] if function == "format" && bang.as_char() == '!' => {
            format_pattern(arguments.stream()).map(|pattern| vec![pattern])
        }
        [TokenTree::Ident(local)] => bindings.get(&local.to_string()).cloned(),
        _ => None,
    }
}
