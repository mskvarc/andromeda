//! Expansion of compact entity type names to IRIs (clause 4.17).

use crate::{
    query_error::QueryError,
    term_expander::TermExpander,
    type_selection::{expanded_type_selection::ExpandedTypeSelection, written_type_selection::WrittenTypeSelection},
};
use std::{future::Future, pin::Pin};

/// Recursively expand a raw type expression, replacing compact type names
/// with their fully expanded IRIs.
///
/// # Errors
///
/// Returns [`QueryError::TermExpansion`] or [`QueryError::TermNotAnIri`] when a
/// type name does not expand to an absolute IRI through the active `@context`.
pub async fn expand_type_selection(raw: &WrittenTypeSelection, expander: &impl TermExpander) -> Result<ExpandedTypeSelection, QueryError> {
    match raw {
        WrittenTypeSelection::Wildcard => Ok(ExpandedTypeSelection::Wildcard),
        WrittenTypeSelection::Type(name) => Ok(ExpandedTypeSelection::Type(expander.expand_type(name.as_str()).await?)),
        WrittenTypeSelection::And(children) => Ok(ExpandedTypeSelection::And(expand_children(children, expander).await?)),
        WrittenTypeSelection::Or(children) => Ok(ExpandedTypeSelection::Or(expand_children(children, expander).await?)),
    }
}

/// Expand every child of a boolean node.
fn expand_children<'a, E: TermExpander>(
    children: &'a [WrittenTypeSelection],
    expander: &'a E,
) -> Pin<Box<dyn Future<Output = Result<Vec<ExpandedTypeSelection>, QueryError>> + Send + 'a>> {
    // Box-pinned: the grammar nests, and an `async fn` cannot name its own
    // future type to recurse through.
    Box::pin(async move {
        let mut expanded_children = Vec::with_capacity(children.len());
        for child in children {
            expanded_children.push(expand_type_selection(child, expander).await?);
        }
        Ok(expanded_children)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::namespace_expander::EXAMPLE_NAMESPACE_TYPED;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    #[tokio::test]
    async fn a_nested_type_selection_expands_at_every_leaf() {
        let raw = WrittenTypeSelection::Or(vec![
            WrittenTypeSelection::And(vec![
                WrittenTypeSelection::Type("Home".parse().expect("valid type name")),
                WrittenTypeSelection::Type("Vehicle".parse().expect("valid type name")),
            ]),
            WrittenTypeSelection::Type("Motorhome".parse().expect("valid type name")),
        ]);
        let expanded = expand_type_selection(&raw, &EXAMPLE_NAMESPACE_TYPED).await.expect("expands");
        assert_eq!(
            expanded,
            ExpandedTypeSelection::Or(vec![
                ExpandedTypeSelection::And(vec![
                    ExpandedTypeSelection::Type(iri("https://example.com/types/Home")),
                    ExpandedTypeSelection::Type(iri("https://example.com/types/Vehicle")),
                ]),
                ExpandedTypeSelection::Type(iri("https://example.com/types/Motorhome")),
            ])
        );
    }
}
