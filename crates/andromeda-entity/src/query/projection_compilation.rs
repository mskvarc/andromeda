//! Compiling raw `pick` / `omit` terms into the clause 6.3.15 projection, for
//! the read path that has no compiled query to take one from.
//!
//! An entity query expands its projection as part of compiling the whole
//! [`CompiledEntityQuery`], and reads it back off that cached compilation. The
//! clause 5.6.3 single-entity retrieval compiles no query at all — it looks an
//! identifier up — yet still carries `pick` and `omit`, so it needs the
//! projection on its own. The expansion itself is shared:
//! [`expand_projection_terms`] is what compiling a query calls too, so the two
//! bindings cannot expand the same terms differently.
//!
//! [`CompiledEntityQuery`]: andromeda_ngsi_ld_query::entity::compiled::CompiledEntityQuery

use crate::{error::EntityError, query::EntityQueryService};
use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_query::projection::{compiled::Projection, expansion::expand_projection_terms, grammar::RawProjectionTerm};

impl EntityQueryService {
    /// Expand a request's `pick` and `omit` terms against the active context.
    ///
    /// Returns `None` when the request named neither, which is what tells the
    /// read path there is nothing to narrow — distinct from a projection whose
    /// term lists happen to be empty.
    ///
    /// # Errors
    ///
    /// Returns [`EntityError`] when a term cannot be expanded through the
    /// context, or when a structural keyword carries a clause 4.21 nested
    /// sub-projection.
    pub async fn compile_projection(
        &self,
        pick: Option<&[RawProjectionTerm]>,
        omit: Option<&[RawProjectionTerm]>,
        context: &ContextSpec,
    ) -> Result<Option<Projection>, EntityError> {
        if pick.is_none() && omit.is_none() {
            return Ok(None);
        }

        let prepared_context = self.engine().prepare_term_context(context);
        let expander = ContextualExpander::new(self.engine(), &prepared_context);
        let pick = expand_projection_terms(pick.unwrap_or(&[]), &expander).await?;
        let omit = expand_projection_terms(omit.unwrap_or(&[]), &expander).await?;

        Ok(Some(Projection::builder().pick(pick).omit(omit).build()))
    }
}

#[cfg(test)]
mod tests {
    use crate::query::service_test_double::service;
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_query::projection::{grammar::RawProjectionTerm, term::ProjectionTerm};
    use iri_rs::IriBuf;
    use serde_json::json;

    fn context() -> ContextSpec {
        ContextSpec::none().with_body_context(Some(json!({
            "hasA": "https://example.com/hasA",
            "temperature": "https://example.com/temperature",
        })))
    }

    fn raw(name: &str, linked: Vec<RawProjectionTerm>) -> RawProjectionTerm {
        RawProjectionTerm {
            name: name.parse().expect("valid attribute name"),
            linked,
        }
    }

    fn iri(name: &str) -> IriBuf {
        IriBuf::new(format!("https://example.com/{name}")).expect("valid IRI")
    }

    /// A request naming neither `pick` nor `omit` restricts nothing, and the
    /// absence has to stay distinguishable from an empty projection so the
    /// renderer does not apply the clause 6.3.15 structural rule to it.
    #[tokio::test]
    async fn a_request_naming_neither_term_list_compiles_to_no_projection() {
        let compiled = service().compile_projection(None, None, &context()).await.expect("compiles");
        assert!(compiled.is_none());
    }

    /// Clause 6.3.15 and clause 4.21: attribute terms expand through the
    /// context, structural keywords stay keywords, and a nested
    /// sub-projection expands with its parent.
    #[tokio::test]
    async fn pick_terms_expand_through_the_context_and_keep_their_nested_terms() {
        let pick = [raw("id", Vec::new()), raw("hasA", vec![raw("temperature", Vec::new())])];

        let compiled = service()
            .compile_projection(Some(&pick), None, &context())
            .await
            .expect("compiles")
            .expect("a named pick compiles to a projection");

        pretty_assertions::assert_eq!(
            compiled.pick,
            vec![
                ProjectionTerm::Id,
                ProjectionTerm::Attribute {
                    iri: iri("hasA"),
                    linked: vec![ProjectionTerm::Attribute {
                        iri: iri("temperature"),
                        linked: Vec::new()
                    }],
                },
            ]
        );
        assert!(compiled.omit.is_empty());
    }

    /// An `omit`-only request compiles to a projection whose `pick` is empty,
    /// which clause 6.3.15 reads as "every member except those omitted".
    #[tokio::test]
    async fn an_omit_only_request_compiles_to_an_empty_pick() {
        let omit = [raw("temperature", Vec::new())];

        let compiled = service()
            .compile_projection(None, Some(&omit), &context())
            .await
            .expect("compiles")
            .expect("a named omit compiles to a projection");

        assert!(compiled.pick.is_empty());
        pretty_assertions::assert_eq!(
            compiled.omit,
            vec![ProjectionTerm::Attribute {
                iri: iri("temperature"),
                linked: Vec::new()
            }]
        );
    }

    /// Clause 4.21: `LinkedEntityTerm` only ever follows `AttrName`, so a
    /// structural keyword carrying `{...}` is a malformed projection.
    #[tokio::test]
    async fn a_structural_keyword_carrying_a_nested_term_is_rejected() {
        let pick = [raw("id", vec![raw("temperature", Vec::new())])];

        assert!(service().compile_projection(Some(&pick), None, &context()).await.is_err());
    }
}
