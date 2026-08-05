//! Expansion of attribute selection lists through the request's `@context`.

use crate::{error::QueryError, projection::term::ProjectionTerm, term_expander::TermExpander};
use iri_rs::IriBuf;

/// Expand a list of short attribute names to their full IRIs.
///
/// Used for every attribute selection list the API accepts: `attrs` on entity
/// and temporal queries, the CSR property/relationship names, and the
/// `expandValues` / `jsonKeys` extensions.
pub async fn expand_attrs(raw: &[String], expander: &impl TermExpander) -> Result<Vec<IriBuf>, QueryError> {
    let mut expanded = Vec::with_capacity(raw.len());
    for term in raw {
        expanded.push(expander.expand_term(term).await?);
    }
    Ok(expanded)
}

/// Expand a `pick` / `omit` term list, keeping the clause 6.3.15 keywords
/// `id`, `type` and `scope` as written.
pub async fn expand_projection_terms(raw: &[String], expander: &impl TermExpander) -> Result<Vec<ProjectionTerm>, QueryError> {
    let mut expanded = Vec::with_capacity(raw.len());
    for term in raw {
        expanded.push(match term.as_str() {
            // Clause 6.3.15: these name structural members of the
            // representation, not attributes, so they have no IRI to expand to.
            ProjectionTerm::ID_KEYWORD => ProjectionTerm::Id,
            ProjectionTerm::TYPE_KEYWORD => ProjectionTerm::Type,
            ProjectionTerm::SCOPE_KEYWORD => ProjectionTerm::Scope,
            // `&str` is unbounded, so a catch-all is the only way to close this
            // match; anything that is not one of the three keywords is an
            // attribute term.
            _ => ProjectionTerm::Attribute(expander.expand_term(term).await?),
        });
    }
    Ok(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct PrefixExpander;

    impl TermExpander for PrefixExpander {
        async fn expand_term(&self, term: &str) -> Result<IriBuf, QueryError> {
            Ok(IriBuf::new(format!("https://example.com/{term}")).expect("valid IRI"))
        }

        async fn expand_type(&self, term: &str) -> Result<IriBuf, QueryError> {
            Ok(IriBuf::new(format!("https://example.com/types/{term}")).expect("valid IRI"))
        }
    }

    #[tokio::test]
    async fn every_attribute_name_is_expanded() {
        let expanded = expand_attrs(&["speed".to_string(), "colour".to_string()], &PrefixExpander)
            .await
            .expect("expands");
        let rendered: Vec<&str> = expanded.iter().map(IriBuf::as_str).collect();
        assert_eq!(rendered, vec!["https://example.com/speed", "https://example.com/colour"]);
    }

    #[tokio::test]
    async fn a_projection_term_of_id_type_or_scope_is_not_expanded() {
        // Clause 6.3.15: `id`, `type` and `scope` name structural members of the
        // representation, not attributes, so they have no IRI to expand to.
        let raw = ["id".to_string(), "type".to_string(), "scope".to_string(), "speed".to_string()];
        let expanded = expand_projection_terms(&raw, &PrefixExpander).await.expect("expands");
        assert_eq!(
            expanded,
            vec![
                ProjectionTerm::Id,
                ProjectionTerm::Type,
                ProjectionTerm::Scope,
                ProjectionTerm::Attribute(IriBuf::new("https://example.com/speed".to_string()).expect("valid IRI")),
            ]
        );
    }
}
