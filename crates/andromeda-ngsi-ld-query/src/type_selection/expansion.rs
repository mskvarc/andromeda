//! Expansion of compact entity type names to IRIs (clause 4.17).

use crate::{
    error::QueryError,
    term_expander::TermExpander,
    type_selection::grammar::{TypeExpr, TypeExprRaw},
};
use std::{future::Future, pin::Pin};

/// Recursively expand a raw type expression, replacing compact type names
/// with their fully expanded IRIs.
pub async fn expand_type_expression(raw: &TypeExprRaw, expander: &impl TermExpander) -> Result<TypeExpr, QueryError> {
    match raw {
        TypeExprRaw::Type(name) => Ok(TypeExpr::Type(expander.expand_type(name).await?)),
        TypeExprRaw::And(children) => Ok(TypeExpr::And(expand_children(children, expander).await?)),
        TypeExprRaw::Or(children) => Ok(TypeExpr::Or(expand_children(children, expander).await?)),
    }
}

/// Expand every child of a boolean node.
fn expand_children<'a, E: TermExpander>(
    children: &'a [TypeExprRaw],
    expander: &'a E,
) -> Pin<Box<dyn Future<Output = Result<Vec<TypeExpr>, QueryError>> + Send + 'a>> {
    // Box-pinned: the grammar nests, and an `async fn` cannot name its own
    // future type to recurse through.
    Box::pin(async move {
        let mut expanded = Vec::with_capacity(children.len());
        for child in children {
            expanded.push(expand_type_expression(child, expander).await?);
        }
        Ok(expanded)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use iri_rs::IriBuf;

    struct PrefixExpander;

    impl TermExpander for PrefixExpander {
        async fn expand_term(&self, term: &str) -> Result<IriBuf, QueryError> {
            Ok(IriBuf::new(format!("https://example.com/{term}")).expect("valid IRI"))
        }

        async fn expand_type(&self, term: &str) -> Result<IriBuf, QueryError> {
            Ok(IriBuf::new(format!("https://example.com/types/{term}")).expect("valid IRI"))
        }
    }

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    #[tokio::test]
    async fn a_nested_type_expression_expands_at_every_leaf() {
        let raw = TypeExprRaw::Or(vec![
            TypeExprRaw::And(vec![TypeExprRaw::Type("Home".to_string()), TypeExprRaw::Type("Vehicle".to_string())]),
            TypeExprRaw::Type("Motorhome".to_string()),
        ]);
        let expanded = expand_type_expression(&raw, &PrefixExpander).await.expect("expands");
        assert_eq!(
            expanded,
            TypeExpr::Or(vec![
                TypeExpr::And(vec![
                    TypeExpr::Type(iri("https://example.com/types/Home")),
                    TypeExpr::Type(iri("https://example.com/types/Vehicle")),
                ]),
                TypeExpr::Type(iri("https://example.com/types/Motorhome")),
            ])
        );
    }
}
