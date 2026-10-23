//! Which local names of one function hold a registered or subscribed entity
//! selector, so that an `id_pattern` read off one of them is told apart from
//! a read of the client's own query.
//!
//! A name holds a selector when it is a parameter typed as one, or when a
//! `let`, a `for` loop or a closure binds it from an expression that reads a
//! block's `entities` or another name already holding one.

use crate::registration_matching::token_reading::mentions;
use quote::ToTokens;
use std::collections::BTreeSet;
use syn::{
    Expr,
    FnArg,
    Pat,
    PatIdent,
    Signature,
    visit::{Visit, visit_pat_ident},
};

/// The types a registered selector is held as, whatever the source imports.
const REGISTERED_SELECTORS: [&str; 1] = ["EntityInfo"];

/// The local names of one function that hold a selector.
#[derive(Default)]
pub(crate) struct SelectorBindings(BTreeSet<String>);

impl SelectorBindings {
    /// The parameters of `signature` typed as a registered selector, or as
    /// the subscription `EntitySelector` when `subscription_selector` names
    /// it in this source.
    pub(crate) fn of_parameters(signature: &Signature, subscription_selector: bool) -> SelectorBindings {
        let mut bindings = SelectorBindings::default();
        for argument in &signature.inputs {
            if let FnArg::Typed(parameter) = argument {
                let declared = parameter.ty.to_token_stream();
                if REGISTERED_SELECTORS.iter().any(|selector| mentions(&declared, selector)) || (subscription_selector && mentions(&declared, "EntitySelector"))
                {
                    bindings.bind(&parameter.pat);
                }
            }
        }
        bindings
    }

    /// Whether an expression yields a selector: it reads a block's
    /// `entities` or a name already holding one.
    pub(crate) fn yields_selector(&self, expression: &Expr) -> bool {
        let tokens = expression.to_token_stream();
        mentions(&tokens, "entities") || self.0.iter().any(|name| mentions(&tokens, name))
    }

    /// Hold every name `pattern` binds.
    pub(crate) fn bind(&mut self, pattern: &Pat) {
        let mut names = BoundNames::default();
        names.visit_pat(pattern);
        self.0.extend(names.0);
    }

    /// Whether `expression` is read off a name holding a selector.
    pub(crate) fn holds(&self, expression: &Expr) -> bool {
        root_binding(expression).is_some_and(|root| self.0.contains(&root))
    }
}

/// The names one pattern binds.
#[derive(Default)]
struct BoundNames(Vec<String>);

impl<'ast> Visit<'ast> for BoundNames {
    fn visit_pat_ident(&mut self, binding: &'ast PatIdent) {
        self.0.push(binding.ident.to_string());
        visit_pat_ident(self, binding);
    }
}

/// The local name an expression is read off, through field reads, method
/// calls, references, dereferences and parentheses.
fn root_binding(expression: &Expr) -> Option<String> {
    // `syn::Expr` has many shapes and only these lead from a read back to the
    // name it starts at, so each is tested for rather than every shape matched.
    if let Expr::Field(read) = expression {
        return root_binding(&read.base);
    }
    if let Expr::MethodCall(call) = expression {
        return root_binding(&call.receiver);
    }
    if let Expr::Reference(reference) = expression {
        return root_binding(&reference.expr);
    }
    if let Expr::Unary(unary) = expression {
        return root_binding(&unary.expr);
    }
    if let Expr::Paren(inner) = expression {
        return root_binding(&inner.expr);
    }
    let Expr::Path(path) = expression else {
        return None;
    };
    path.path.get_ident().map(ToString::to_string)
}
