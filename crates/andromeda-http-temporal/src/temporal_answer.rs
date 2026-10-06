//! How a temporal read writes the evolutions it answers with.
//!
//! Every read endpoint resolves the same selectors before it renders -- the
//! clause 6.3.12 representation, the natural-language filter, and the
//! compiled clause 4.21 projection -- and a read that asked for a clause 4.5.23
//! join then puts the Linked Entities into the answer in whichever shape the
//! join mode names. Neither is the property of any one handler, so both are
//! written once here.
//!
//! The evolutions are written straight from their typed form into the response
//! body, the Linked Entities of a `join=inline` nested into them as they are
//! written.

use crate::error::TemporalResultExt;
use andromeda_http_response::{problem_response::ProblemResponse, rendered_response::RenderedResponse};
use andromeda_linked_retrieval::plan::JoinPlan;
use andromeda_ngsi_ld_model::temporal::temporal_body::TemporalBody;
use andromeda_ngsi_ld_render::{
    response::ResponseRenderer,
    structural_projection::StructuralProjection,
    temporal::{
        temporal_evolutions::TemporalEvolutions,
        temporal_response::{TemporalPayload, TemporalRepresentation, TemporalRoot},
    },
};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use axum::http::Uri as HttpUri;
use std::slice::from_ref;

/// What a temporal read answers with: typed evolutions, written straight into
/// the body.
pub(crate) struct TemporalAnswer<'a>(TemporalPayload<'a>);

impl TemporalAnswer<'_> {
    /// Render the answer under `rendering`, in the representation and under
    /// the language filter `params` selected.
    ///
    /// # Errors
    ///
    /// Returns a [`ProblemResponse`] when an evolution cannot be written in the
    /// requested representation.
    pub(crate) fn render(
        self,
        rendering: ResponseRenderer<'_>,
        params: &TemporalQueryParameters,
        instance: &HttpUri,
    ) -> Result<RenderedResponse, ProblemResponse> {
        let representation = TemporalRepresentation {
            format: params.selected_format(),
            timeproperty: params.timeproperty_or_default(),
        };
        RenderedResponse::temporal(rendering.with_temporal(self.0, representation, params.lang.as_ref())).with_instance(instance)
    }
}

/// What a retrieval writes into the response body.
pub(crate) enum RetrievedAnswer<'a> {
    /// The evolution itself (clause 6.5.3.1).
    Single(TemporalAnswer<'a>),
    /// The evolution beside the Linked Entities a flat join reached.
    Collection(TemporalAnswer<'a>),
}

/// The answer to a query that matched `bodies`, together with the Linked
/// Entities the engine's plan carries, in the shape the join mode names.
///
/// Without a join the matched evolutions are the collection. Flat puts the
/// roots ahead of the planned Linked Entities in one collection, each written
/// by the rule its first encounter decided (clause 5.7.1.5); inline nests into
/// each root what the plan's cursor answers.
pub(crate) fn matched_histories<'a>(
    bodies: &'a [TemporalBody],
    plan: Option<&'a JoinPlan<'a, TemporalEvolutions>>,
    projection: Option<StructuralProjection<'a>>,
) -> TemporalAnswer<'a> {
    let roots = bodies.iter().map(move |body| TemporalRoot::new(body, projection));
    TemporalAnswer(TemporalPayload::Collection(match plan {
        None => roots.collect(),
        Some(JoinPlan::Flat(flat)) => roots.chain(flat.linked().map(TemporalRoot::linked)).collect(),
        Some(JoinPlan::Inline(inline)) => roots.map(|root| root.with_inline_join(inline.cursor())).collect(),
    }))
}

/// The answer to a retrieval of `body`: the evolution itself, nested with the
/// Linked Entities of an inline join, or -- for a flat join, which answers a
/// collection even though one entity was asked for (clause 4.5.23) -- the
/// evolution beside them.
pub(crate) fn retrieved_history<'a>(
    body: &'a TemporalBody,
    plan: Option<&'a JoinPlan<'a, TemporalEvolutions>>,
    projection: Option<StructuralProjection<'a>>,
) -> RetrievedAnswer<'a> {
    match plan {
        None => RetrievedAnswer::Single(TemporalAnswer(TemporalPayload::Single(TemporalRoot::new(body, projection)))),
        Some(JoinPlan::Flat(_)) => RetrievedAnswer::Collection(matched_histories(from_ref(body), plan, projection)),
        Some(JoinPlan::Inline(inline)) => RetrievedAnswer::Single(TemporalAnswer(TemporalPayload::Single(
            TemporalRoot::new(body, projection).with_inline_join(inline.cursor()),
        ))),
    }
}
