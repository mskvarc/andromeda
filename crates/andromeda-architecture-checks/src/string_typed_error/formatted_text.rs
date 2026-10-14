//! Where text built with `format!` sits inside an expression.
//!
//! Text reaches an error carrier through more than the macro call itself: a
//! local bound to it earlier in the function, a helper that returns it, or any
//! wrapping on the way (`Some(..)`, `.into()`, a `match` arm). The search looks
//! at every node of the expression, so each of those shapes is found where it
//! stands.

use crate::string_typed_error::workspace_index::{WorkspaceIndex, is_formatting_macro};
use std::collections::BTreeSet;
use syn::{
    Expr,
    Ident,
    Pat,
    visit::{Visit, visit_expr},
};

/// The knowledge a search for formatted text needs at one point of a walk.
#[derive(Clone, Copy)]
pub(crate) struct FormattedText<'a> {
    /// What the workspace declares about functions that return formatted text.
    pub(crate) index: &'a WorkspaceIndex,
    /// Locals of the enclosing function bound to formatted text.
    pub(crate) locals: &'a BTreeSet<String>,
}

impl FormattedText<'_> {
    /// The first node, in source order, of `expr` that is formatted text.
    pub(crate) fn first_within(self, expr: &Expr) -> Option<&Expr> {
        let mut finder = FormattedTextFinder { text: self, found: None };
        finder.visit_expr(expr);
        finder.found
    }

    /// Whether one node is formatted text in its own right: a formatting macro,
    /// a local bound to one, or a call of a function that returns one.
    fn is_formatted(self, expr: &Expr) -> bool {
        if let Expr::Macro(macro_expr) = expr {
            return is_formatting_macro(&macro_expr.mac.path);
        }
        if let Expr::Path(path) = expr {
            return path.qself.is_none() && path.path.get_ident().is_some_and(|ident| self.locals.contains(&ident.to_string()));
        }
        if let Expr::MethodCall(call) = expr {
            return self.index.method_formats_text(&call.method);
        }
        if let Expr::Call(call) = expr
            && let Expr::Path(function) = call.func.as_ref()
        {
            return function
                .path
                .segments
                .last()
                .is_some_and(|segment| self.index.function_formats_text(&segment.ident));
        }
        false
    }
}

/// Finds the first formatted node below the expression it visits.
struct FormattedTextFinder<'a, 'e> {
    /// What counts as formatted text.
    text: FormattedText<'a>,
    /// The node found, once found.
    found: Option<&'e Expr>,
}

impl<'e> Visit<'e> for FormattedTextFinder<'_, 'e> {
    fn visit_expr(&mut self, expr: &'e Expr) {
        if self.found.is_some() {
            return;
        }
        if self.text.is_formatted(expr) {
            self.found = Some(expr);
            return;
        }
        visit_expr(self, expr);
    }
}

/// Every name a `let` pattern binds, through tuples, structs, references and
/// type ascriptions.
///
/// A destructured `match` hands each name its part of the arm, so every name
/// is taken to hold what the initializer built.
pub(crate) fn bound_idents<'p>(pattern: &'p Pat, bound: &mut Vec<&'p Ident>) {
    if let Pat::Ident(pattern) = pattern {
        bound.push(&pattern.ident);
    }
    if let Pat::Type(typed) = pattern {
        bound_idents(&typed.pat, bound);
    }
    if let Pat::Reference(reference) = pattern {
        bound_idents(&reference.pat, bound);
    }
    if let Pat::Paren(inner) = pattern {
        bound_idents(&inner.pat, bound);
    }
    if let Pat::Tuple(tuple) = pattern {
        tuple.elems.iter().for_each(|element| bound_idents(element, bound));
    }
    if let Pat::TupleStruct(tuple) = pattern {
        tuple.elems.iter().for_each(|element| bound_idents(element, bound));
    }
    if let Pat::Struct(fields) = pattern {
        fields.fields.iter().for_each(|field| bound_idents(&field.pat, bound));
    }
}
