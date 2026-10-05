//! How a read writes the entities it answers with.
//!
//! Every read endpoint resolves the same three clause 4.5 selectors before it
//! renders — the natural-language filter, the representation, and the compiled
//! clause 4.21 projection — and a read that asked for a clause 4.5.23 join then
//! puts the Linked Entities into the answer in whichever shape the join mode
//! names. Neither is the property of any one handler, so both are written once
//! here.
//!
//! The entities are written straight from their typed form into the response
//! body, the Linked Entities of a `join=inline` nested into them as they are
//! written.

use crate::{error::EntityResultExt, join::ResolvedJoin};
use andromeda_entity::query::{EntityQueryService, simplified_names::apply_simplified_names};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_http_response::{problem_response::ProblemResponse, rendered_response::RenderedResponse};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_linked_retrieval::join::RetrievingJoin;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::projection::compiled::Projection;
use andromeda_ngsi_ld_render::{
    entity_response::{EntityBody, EntityPayload, InlineJoin},
    response::ResponseRenderer,
    structural_projection::StructuralProjection,
};
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use axum::http::Uri as HttpUri;
use std::slice::from_ref;

/// The clause 4.5 selectors a read renders its entities under, beside the
/// clause 4.15 natural-language selection the response renderer applies.
pub(crate) struct EntityRepresentation<'a> {
    /// The clause 4.5.1 representation the response is written in.
    pub(crate) mode: ResponseFormat,
    /// The compiled clause 4.21 projection, absent when the request narrows no member.
    pub(crate) projection: Option<&'a Projection>,
}

impl<'a> EntityRepresentation<'a> {
    /// Gather the selectors a read resolved from its request.
    pub(crate) const fn new(mode: ResponseFormat, projection: Option<&'a Projection>) -> EntityRepresentation<'a> {
        EntityRepresentation { mode, projection }
    }

    /// The clause 4.21 rule the renderer applies to `id`, `type` and `scope`.
    pub(crate) fn structural_projection(&self) -> Option<StructuralProjection<'a>> {
        self.projection.map(StructuralProjection::from)
    }

    /// One entity the request selected, narrowed by the request's own rule.
    fn body<'e>(&self, entity: &'e Entity) -> EntityBody<'e>
    where
        'a: 'e,
    {
        EntityBody::new(entity, self.structural_projection())
    }

    /// One entity the request selected, narrowed by the request's own rule,
    /// the Linked Entities of an inline join nested into it (clause 4.5.23),
    /// each narrowed by the sub-projection the naming Relationship carried
    /// (clause 4.21).
    fn inline_body<'e>(&self, entity: &'e Entity, resolved: &'e ResolvedJoin<'_>) -> EntityBody<'e>
    where
        'a: 'e,
    {
        self.body(entity).with_inline_join(InlineJoin {
            linked: &resolved.linked,
            projection: resolved.linked_projection(),
        })
    }
}

/// Give every entity the Attribute names the representation about to be
/// rendered uses.
///
/// Clause C.6: a `@context` term that coerces a datatype cannot name a reified
/// Property, so the normalized and concise representations keep the Fully
/// Qualified Name, while "in the simplified (keyValue) representation case,
/// such an @context with the specification of testedAt could be used, as there
/// is no reification". Every other representation renders the names the
/// Entity's own compaction produced, so nothing is done for them.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when the request context cannot be processed,
/// or when it declares a term that is not a name the clause 4.6.2 grammar
/// admits.
pub(crate) async fn apply_representation_names(
    service: &EntityQueryService,
    representation: &EntityRepresentation<'_>,
    context: &ContextSpec,
    entities: &mut [Entity],
    resolved: Option<&mut ResolvedJoin<'_>>,
    instance: &HttpUri,
) -> Result<(), ProblemResponse> {
    if !representation.mode.is_simplified() {
        return Ok(());
    }
    let names = service.name_compaction(context).await.with_entity_instance(instance)?;
    let linked = resolved.into_iter().flat_map(|resolved| resolved.linked.values_mut());
    for entity in entities.iter_mut().chain(linked) {
        apply_simplified_names(entity, &names).with_entity_instance(instance)?;
    }
    Ok(())
}

/// What a read answers with: typed entities, written straight into the body.
pub(crate) struct ReadAnswer<'a>(EntityPayload<'a>);

impl ReadAnswer<'_> {
    /// Render the answer under `rendering`.
    ///
    /// # Errors
    ///
    /// Returns a [`ProblemResponse`] when an entity cannot be written in the
    /// requested representation.
    pub(crate) fn render(
        self,
        rendering: ResponseRenderer<'_>,
        language_filter: Option<&LanguageFilter>,
        instance: &HttpUri,
    ) -> Result<RenderedResponse, ProblemResponse> {
        RenderedResponse::entities(rendering.with_entities(self.0, language_filter)).with_entity_instance(instance)
    }
}

/// The answer to a read that matched `entities`, together with the Linked
/// Entities a clause 4.5.23 join reached, in the shape the join mode names.
///
/// Without a join the matched entities are the collection. Flat puts the roots
/// ahead of the linked entities in one collection, each linked entity narrowed
/// by the sub-projection the naming Relationship carried (clause 5.7.1.5);
/// inline nests each root's own into it.
pub(crate) fn matched_entities<'a>(
    entities: &'a [Entity],
    resolved: Option<&'a ResolvedJoin<'_>>,
    representation: &EntityRepresentation<'a>,
) -> ReadAnswer<'a> {
    let roots = entities.iter().map(|entity| representation.body(entity));
    ReadAnswer(EntityPayload::Collection(match resolved {
        None => roots.collect(),
        Some(resolved) => match resolved.join {
            RetrievingJoin::Flat => roots
                .chain(resolved.flat_linked().map(|(entity, projection)| EntityBody::new(entity, projection)))
                .collect(),
            RetrievingJoin::Inline => entities.iter().map(|entity| representation.inline_body(entity, resolved)).collect(),
        },
    }))
}

/// The answer to a retrieval of `entity`: the entity itself, nested with the
/// Linked Entities of an inline join, or -- for a flat join, which answers a
/// collection even though one entity was asked for (clause 4.5.23) -- the
/// entity beside them.
pub(crate) fn retrieved_entity<'a>(
    entity: &'a Entity,
    resolved: Option<&'a ResolvedJoin<'_>>,
    representation: &EntityRepresentation<'a>,
) -> RetrievedAnswer<'a> {
    match resolved {
        None => RetrievedAnswer::Single(ReadAnswer(EntityPayload::Single(representation.body(entity)))),
        Some(resolved) => match resolved.join {
            RetrievingJoin::Flat => RetrievedAnswer::Collection(matched_entities(from_ref(entity), Some(resolved), representation)),
            RetrievingJoin::Inline => RetrievedAnswer::Single(ReadAnswer(EntityPayload::Single(representation.inline_body(entity, resolved)))),
        },
    }
}

/// What a retrieval writes into the response body.
pub(crate) enum RetrievedAnswer<'a> {
    /// The entity itself (clause 6.5.3.1).
    Single(ReadAnswer<'a>),
    /// The entity beside the Linked Entities a flat join reached.
    Collection(ReadAnswer<'a>),
}
