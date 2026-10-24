//! The clause 4.21 projection a query request compiled to.
//!
//! Both query endpoints already have the request compiled — `EntityQueryService`
//! caches a [`CompiledEntityQuery`] keyed on the parameters and the context
//! fingerprint, and its `projection` is the very thing the response has to be
//! narrowed by. Reading it back off that cached compilation is what stops the
//! handlers from expanding the same `pick`/`omit` terms a second time.
//!
//! The compilation is skipped altogether when the request named none of
//! `pick`, `omit` and `attrs`: with nothing to narrow, a cache lookup that
//! clones a whole compiled query would be pure waste.

use andromeda_entity::{error::EntityError, query::EntityQueryService};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_linked_retrieval::narrowing::list::ListInForce;
use andromeda_ngsi_ld_query::{entity::compiled::CompiledEntityQuery, projection::compiled::Projection};
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;

/// The compiled query a response's projection is read from, held so the
/// borrows handed to the renderer and the join outlive it.
pub(crate) struct RequestProjection {
    /// The compiled query, absent when the request named no `pick`, `omit` or
    /// `attrs`.
    compiled: Option<CompiledEntityQuery>,
}

impl RequestProjection {
    /// Compile the request, or establish that it selects every member.
    ///
    /// # Errors
    ///
    /// Returns [`EntityError`] when the query cannot be compiled against the
    /// active context.
    pub(crate) async fn of(service: &EntityQueryService, params: &EntityQueryParameters, context: &ContextSpec) -> Result<RequestProjection, EntityError> {
        let compiled = match (params.pick.as_ref(), params.omit.as_ref(), params.attrs.as_ref()) {
            (None, None, None) => None,
            (Some(_), _, _) | (_, Some(_), _) | (_, _, Some(_)) => Some(service.compile_query_public(params, context, service.language_targeting()).await?),
        };
        Ok(RequestProjection { compiled })
    }

    /// The clause 4.21 projection, or `None` when the request named neither
    /// `pick` nor `omit`.
    pub(crate) fn projection(&self) -> Option<&Projection> {
        self.compiled
            .as_ref()
            .map(|compiled| &compiled.projection)
            .filter(|projection| !projection.is_empty())
    }

    /// The list the request narrows every entity of the payload by, Linked
    /// Entities included (clause 5.7.2.5).
    pub(crate) fn list(&self) -> ListInForce<'_> {
        match &self.compiled {
            Some(compiled) => ListInForce::of(Some(&compiled.projection), &compiled.selection.attrs),
            None => ListInForce::Unrestricted,
        }
    }
}
