//! A query's entity selectors with their type expressions expanded, as the
//! registration preselect reads them (clause 5.12).
//!
//! Registrations hold the types they cover expanded, so each selector of the
//! union a client asked for (clause 5.2.33) is matched with its clause 4.17
//! type expression expanded under the request context, whatever its shape.

use crate::error::FederationError;
use andromeda_jsonld::{
    engine::{processor::JsonLdEngine, term_lookup::PreparedTermContext},
    term_expansion::ContextualExpander,
};
use andromeda_ngsi_ld_query::{
    csr::expanded_entity_specification::ExpandedEntitySpecification,
    expanded_entity_selector::ExpandedEntitySelector,
    selector_compilation::compile_entity_selectors,
    written_entity_selector::WrittenEntitySelector,
};

/// The selectors of one read, each compiled with its type expression expanded.
pub(crate) struct ExpandedSelectors {
    /// The compiled selectors, in the order the client wrote them.
    selectors: Vec<ExpandedEntitySelector>,
}

impl ExpandedSelectors {
    /// Compile every selector under `context`.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::Query`] when a type expression has a term
    /// that cannot be expanded against the request context.
    pub(crate) async fn expand(
        engine: &JsonLdEngine,
        context: &PreparedTermContext,
        selectors: &[WrittenEntitySelector],
    ) -> Result<ExpandedSelectors, FederationError> {
        let selectors = compile_entity_selectors(selectors, &ContextualExpander::new(engine, context)).await?;
        Ok(ExpandedSelectors { selectors })
    }

    /// The clause 5.12 specification each selector is: its expanded type, its
    /// identifiers and its identifier pattern.
    pub(crate) fn specifications(&self) -> Vec<ExpandedEntitySpecification<'_>> {
        self.selectors
            .iter()
            .map(|selector| ExpandedEntitySpecification {
                entity_type: selector.type_selection.as_ref(),
                ids: &selector.ids,
                id_pattern: selector.id_pattern.as_ref(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::{fixed_registration_store::offline_service, selector_expansion::ExpandedSelectors};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_query::{
        type_selection::{expanded_type_selection::ExpandedTypeSelection, written_type_selection::WrittenTypeSelection},
        written_entity_selector::WrittenEntitySelector,
    };
    use serde_json::json;

    fn selector(type_: &str) -> WrittenEntitySelector {
        WrittenEntitySelector {
            type_selection: Some(type_.parse::<WrittenTypeSelection>().expect("a valid type expression")),
            ..WrittenEntitySelector::default()
        }
    }

    async fn specified(selectors: &[WrittenEntitySelector]) -> Vec<Option<String>> {
        let service = offline_service(Vec::new());
        let context = ContextSpec::from_body(
            JsonLiteral::admit(&json!({"Alpha": "https://example.org/Alpha", "Beta": "https://example.org/Beta"}).to_string()).expect("an admissible literal"),
        );
        let prepared = service.engine().prepare_term_context(&context);
        let expanded = ExpandedSelectors::expand(service.engine(), &prepared, selectors)
            .await
            .expect("the selectors expand");
        expanded
            .specifications()
            .into_iter()
            .map(|specification| specification.entity_type.map(|expression| format!("{expression:?}")))
            .collect()
    }

    /// Clause 5.2.33: each selector of the union keeps its own type, and a
    /// selector naming none stays unconstrained on the type axis.
    #[tokio::test]
    async fn each_selector_keeps_its_own_expanded_type() {
        let specifications = specified(&[selector("Alpha"), selector("Beta"), WrittenEntitySelector::default()]).await;
        assert!(
            specifications[0].as_deref().is_some_and(|text| text.contains("https://example.org/Alpha")),
            "{specifications:?}"
        );
        assert!(
            specifications[1].as_deref().is_some_and(|text| text.contains("https://example.org/Beta")),
            "{specifications:?}"
        );
        pretty_assertions::assert_eq!(specifications[2], None);
    }

    /// Clause 4.17: an alternation or a conjunction is expanded term by term
    /// rather than dropped from the preselect.
    #[tokio::test]
    async fn a_boolean_type_expression_is_expanded_in_full() {
        let specifications = specified(&[selector("Alpha|Beta"), selector("(Alpha;Beta)")]).await;
        for specification in specifications {
            let text = specification.expect("the expression is kept");
            assert!(
                text.contains("https://example.org/Alpha") && text.contains("https://example.org/Beta"),
                "{text}"
            );
        }
    }

    #[tokio::test]
    async fn a_wildcard_type_expands_to_the_wildcard() {
        let service = offline_service(Vec::new());
        let prepared = service.engine().prepare_term_context(&ContextSpec::none());
        let selectors = [selector("*")];
        let expanded = ExpandedSelectors::expand(service.engine(), &prepared, &selectors)
            .await
            .expect("the selectors expand");
        assert!(matches!(expanded.specifications()[0].entity_type, Some(ExpandedTypeSelection::Wildcard)));
    }
}
