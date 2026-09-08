//! How a read writes the entities it answers with.
//!
//! Every read endpoint resolves the same three clause 4.5 selectors before it
//! renders — the natural-language filter, the representation, and the compiled
//! clause 4.21 projection — and a read that asked for a clause 4.5.23 join then
//! puts the Linked Entities into the answer in whichever shape the join mode
//! names. Neither is the property of any one handler, so both are written once
//! here.

use crate::{error::EntityResultExt, join::ResolvedJoin};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::projection::compiled::Projection;
use andromeda_ngsi_ld_render::entity::{EntityRenderer, render_flat_join};
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use andromeda_ngsi_ld_request::join_traversal::RetrievingJoin;
use axum::http::Uri as HttpUri;
use serde_json::Value;
use std::iter::empty;

/// The clause 4.5 selectors a read renders its entities under.
pub(crate) struct EntityRepresentation<'a> {
    /// The clause 4.15 natural-language selection, when the request sent one.
    pub(crate) language_filter: Option<&'a LanguageFilter>,
    /// The clause 4.5.1 representation the response is written in.
    pub(crate) mode: ResponseFormat,
    /// The compiled clause 4.21 projection, absent when the request narrows no member.
    pub(crate) projection: Option<&'a Projection>,
}

impl<'a> EntityRepresentation<'a> {
    /// Gather the three selectors a read resolved from its request.
    pub(crate) const fn new(language_filter: Option<&'a LanguageFilter>, mode: ResponseFormat, projection: Option<&'a Projection>) -> EntityRepresentation<'a> {
        EntityRepresentation {
            language_filter,
            mode,
            projection,
        }
    }
}

/// Render the entities a read matched, without a join.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when an entity cannot be written in the
/// requested representation.
pub(crate) fn render_entities(entities: &[Entity], representation: &EntityRepresentation<'_>, instance: &HttpUri) -> Result<Vec<Value>, ProblemResponse> {
    render_flat_join(
        entities.iter(),
        empty(),
        representation.language_filter,
        representation.mode,
        representation.projection,
    )
    .with_entity_instance(instance)
}

/// Render the single entity a retrieval answers with, nesting the Linked
/// Entities of an inline join into it when the read resolved one (clause
/// 4.5.23), each narrowed by the sub-projection the naming Relationship
/// carried (clause 4.21).
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when the entity cannot be written in the
/// requested representation.
pub(crate) fn render_entity(
    entity: &Entity,
    inline: Option<&ResolvedJoin<'_>>,
    representation: &EntityRepresentation<'_>,
    instance: &HttpUri,
) -> Result<Value, ProblemResponse> {
    let mut renderer = EntityRenderer::new(entity)
        .with_optional_language_filter(representation.language_filter)
        .with_optional_structural_projection(representation.projection)
        .with_mode(representation.mode);
    if let Some(resolved) = inline {
        renderer = renderer
            .with_inline_linked_entities(&resolved.linked)
            .with_optional_linked_projection(resolved.linked_projection());
    }
    renderer.render().with_entity_instance(instance)
}

/// Render the entities a read matched together with the Linked Entities the
/// join reached, in the shape the clause 4.5.23 mode names.
///
/// Flat puts the roots ahead of the linked entities in one collection; inline
/// nests each root's own into it, narrowed by the sub-projection the naming
/// Relationship carried (clause 4.21).
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when an entity cannot be written in the
/// requested representation.
pub(crate) fn render_joined_entities(
    entities: &[Entity],
    resolved: &ResolvedJoin<'_>,
    representation: &EntityRepresentation<'_>,
    instance: &HttpUri,
) -> Result<Vec<Value>, ProblemResponse> {
    match resolved.join {
        RetrievingJoin::Flat => render_flat_join(
            entities.iter(),
            resolved.linked.values(),
            representation.language_filter,
            representation.mode,
            representation.projection,
        )
        .with_entity_instance(instance),
        RetrievingJoin::Inline => entities
            .iter()
            .map(|entity| {
                EntityRenderer::new(entity)
                    .with_optional_language_filter(representation.language_filter)
                    .with_inline_linked_entities(&resolved.linked)
                    .with_optional_linked_projection(resolved.linked_projection())
                    .with_optional_structural_projection(representation.projection)
                    .with_mode(representation.mode)
                    .render()
                    .with_entity_instance(instance)
            })
            .collect(),
    }
}
