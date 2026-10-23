//! What an expression's tokens name: an identifier, an `expiresAt` read, the
//! current instant, the active subscription status, an ordering comparison.

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use quote::ToTokens;
use syn::{
    BinOp,
    Expr,
    ExprBinary,
    visit::{Visit, visit_expr_binary},
};

/// The members whose `expires_at` is the expiry of a registration or a
/// subscription.
const EXPIRY_OWNERS: [&str; 2] = ["lifecycle", "standing"];

/// Whether `op` orders its operands.
pub(crate) const fn is_order_comparison(op: BinOp) -> bool {
    matches!(op, BinOp::Lt(_) | BinOp::Le(_) | BinOp::Gt(_) | BinOp::Ge(_))
}

/// The tokens of the first ordering comparison inside a call argument, such
/// as the body of an `is_none_or` closure.
pub(crate) fn order_comparison(argument: &Expr) -> Option<TokenStream> {
    let mut comparison = FirstComparison::default();
    comparison.visit_expr(argument);
    comparison.0
}

/// Whether `tokens` hold the identifier `name` at any depth.
pub(crate) fn mentions(tokens: &TokenStream, name: &str) -> bool {
    flattened(tokens).iter().any(|token| token == name)
}

/// Whether `tokens` read the `expires_at` of a registration lifecycle or a
/// subscription standing.
pub(crate) fn reads_owned_expiry(tokens: &TokenStream) -> bool {
    flattened(tokens)
        .windows(3)
        .any(|window| EXPIRY_OWNERS.contains(&window[0].as_str()) && window[1] == "." && window[2] == "expires_at")
}

/// Whether `tokens` read an `expires_at` field, not call an `expires_at`
/// method.
pub(crate) fn reads_expiry_field(tokens: &TokenStream) -> bool {
    let flat = flattened(tokens);
    flat.windows(2)
        .enumerate()
        .any(|(index, pair)| pair[0] == "." && pair[1] == "expires_at" && flat.get(index + 2).is_none_or(|next| next != "("))
}

/// Whether `tokens` call `Utc::now`.
pub(crate) fn names_current_instant(tokens: &TokenStream) -> bool {
    names_path(tokens, "Utc", "now")
}

/// Whether `tokens` spell `SubscriptionStatus::Active` at any depth.
pub(crate) fn names_active_status(tokens: &TokenStream) -> bool {
    names_path(tokens, "SubscriptionStatus", "Active")
}

/// Whether `tokens` spell the path `owner::item` at any depth.
fn names_path(tokens: &TokenStream, owner: &str, item: &str) -> bool {
    flattened(tokens)
        .windows(4)
        .any(|window| window[0] == owner && window[1] == ":" && window[2] == ":" && window[3] == item)
}

/// The first ordering comparison found below an expression.
#[derive(Default)]
struct FirstComparison(Option<TokenStream>);

impl<'ast> Visit<'ast> for FirstComparison {
    fn visit_expr_binary(&mut self, expression: &'ast ExprBinary) {
        if self.0.is_none() && is_order_comparison(expression.op) {
            self.0 = Some(expression.to_token_stream());
        }
        visit_expr_binary(self, expression);
    }
}

/// The tokens of `tokens` as text, each group opened between its delimiters
/// so that a call's parentheses stay visible.
fn flattened(tokens: &TokenStream) -> Vec<String> {
    tokens
        .clone()
        .into_iter()
        .flat_map(|token| match token {
            TokenTree::Group(group) => {
                let inner = flattened(&group.stream());
                match delimiters(group.delimiter()) {
                    Some((open, close)) => [open.to_owned()].into_iter().chain(inner).chain([close.to_owned()]).collect(),
                    None => inner,
                }
            }
            TokenTree::Ident(_) | TokenTree::Punct(_) | TokenTree::Literal(_) => vec![token.to_string()],
        })
        .collect()
}

/// The text a group delimiter opens and closes with; `None` for an
/// invisible group.
const fn delimiters(delimiter: Delimiter) -> Option<(&'static str, &'static str)> {
    match delimiter {
        Delimiter::Parenthesis => Some(("(", ")")),
        Delimiter::Brace => Some(("{", "}")),
        Delimiter::Bracket => Some(("[", "]")),
        Delimiter::None => None,
    }
}
