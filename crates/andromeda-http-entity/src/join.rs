//! Resolving a clause 4.5.23 join for a read response.
//!
//! All three read endpoints — `GET /entities`, `GET /entities/{id}` and
//! `POST /entityOperations/query` — follow the same two steps before they
//! render: traverse to the linked entities, then prepare the clause 4.21
//! nested sub-projections in whichever shape the join mode applies them.
//! None of the handlers performs either itself; they describe the join with a
//! [`JoinRequest`], ask for a [`ResolvedJoin`], and render what it holds.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    error::EntityError,
    query::{EntityQueryService, linked_entity::ResolvedLinkedEntities},
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::projection::{attribute_iri_index::AttributeIriIndex, compiled::Projection};
use andromeda_ngsi_ld_render::nested_projection::LinkedProjection;
use andromeda_ngsi_ld_representation::{linked_resolution_failure::LinkedResolutionFailure, sys_attrs::SysAttrsInclusion};
use andromeda_ngsi_ld_request::join_traversal::{RetrievingJoin, traversal_bounds};
use indexmap::IndexMap;
use std::num::NonZeroU32;

/// The clause 4.5.23 traversal a read request asked for.
///
/// Every read endpoint spells these five out with its own parameter type;
/// gathering them here is what lets the traversal itself be written once.
pub(crate) struct JoinRequest<'a> {
    /// How the linked entities reach the response.
    pub(crate) join: RetrievingJoin,
    /// How many links deep the traversal goes; absent means the clause 4.5.23
    /// default depth.
    pub(crate) join_level: Option<NonZeroU32>,
    /// The entities the client already holds, which the traversal never
    /// retrieves again.
    pub(crate) contained_by: Option<&'a [Uri]>,
    /// Whether the linked entities carry the clause 4.8 system timestamps.
    pub(crate) sys_attrs: SysAttrsInclusion,
    /// The request's compiled clause 4.21 projection, whose terms may carry
    /// the clause 4.21 sub-projections that narrow the linked entities.
    pub(crate) projection: Option<&'a Projection>,
}

/// A resolved join, ready to render.
pub(crate) struct ResolvedJoin<'a> {
    /// The linked entities in breadth-first traversal order, already narrowed
    /// when the join mode narrows them ahead of rendering.
    pub(crate) linked: IndexMap<Uri, Entity>,
    /// The Linked Entities the traversal could not resolve. Each becomes a
    /// clause 6.3.17 warning on the response the handler renders.
    pub(crate) failures: Vec<LinkedResolutionFailure>,
    /// The projection an inline rendering walks for its sub-projections.
    projection: Option<&'a Projection>,
    /// The attribute-key expansions an inline rendering matches terms with,
    /// absent whenever nothing is left for the renderer to narrow.
    index: Option<AttributeIriIndex>,
}

impl<'a> ResolvedJoin<'a> {
    /// Traverse to the linked entities and prepare the nested projections.
    ///
    /// Only a [`RetrievingJoin`] gets here: a request that asks for no join
    /// retrieves no linked entity, so the caller skips this entirely rather
    /// than paying for a traversal it discards.
    ///
    /// # Errors
    ///
    /// Returns [`EntityError`] when the store cannot be read or a nested
    /// projection cannot be matched against the entities' attribute keys.
    pub(crate) async fn prepare(
        service: &EntityQueryService,
        target: &ContextTarget,
        roots: &[Entity],
        context: &ContextSpec,
        request: &JoinRequest<'a>,
    ) -> Result<ResolvedJoin<'a>, EntityError> {
        let (join_level, contained_by) = traversal_bounds(request.join_level, request.contained_by);

        let ResolvedLinkedEntities {
            entities: mut linked,
            failures,
        } = service
            .resolve_linked_entities(target, roots, join_level, request.sys_attrs, context, &contained_by)
            .await?;

        // Clause 4.21: a `pick`/`omit` term may carry a nested sub-projection
        // governing how the entities it links to render.
        let index = match request.projection {
            Some(projection) => service.prepare_join_projection(roots, &mut linked, projection, context, request.join).await?,
            None => None,
        };

        Ok(ResolvedJoin {
            linked,
            failures,
            projection: request.projection,
            index,
        })
    }

    /// The nested sub-projections an inline rendering applies, if the join mode
    /// left any for the renderer.
    pub(crate) fn linked_projection(&self) -> Option<LinkedProjection<'_>> {
        let projection = self.projection?;
        let index = self.index.as_ref()?;
        Some(LinkedProjection { projection, index })
    }
}
