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
    query::{EntityQueryService, join_projection::PreparedJoinProjection, linked_entity::ResolvedLinkedEntities},
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::projection::compiled::Projection;
use andromeda_ngsi_ld_render::{nested_projection::LinkedProjection, structural_projection::StructuralProjection};
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, linked_resolution_failure::LinkedResolutionFailure, sys_attrs::SysAttrsInclusion};
use andromeda_ngsi_ld_request::{
    join_traversal::{RetrievingJoin, traversal_bounds},
    query::entity::parameters::EntityQueryParameters,
};
use indexmap::IndexMap;
use std::num::NonZeroU32;

/// The clause 4.5.23 traversal a read request asked for.
///
/// Every read endpoint spells these five out with its own parameter type;
/// gathering them here is what lets the traversal itself be written once.
pub(crate) struct JoinRequest<'a> {
    /// How the linked entities reach the response, as the request wrote it:
    /// `@none` and an absent `join` ask for no traversal at all.
    pub(crate) join: Option<JoinMode>,
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
    /// The mode the linked entities reach the response in, which the rendering
    /// reads back off the resolution rather than being told a second time.
    pub(crate) join: RetrievingJoin,
    /// The linked entities in breadth-first traversal order, already narrowed
    /// when the join mode narrows them ahead of rendering.
    pub(crate) linked: IndexMap<Uri, Entity>,
    /// The Linked Entities the traversal could not resolve. Each becomes a
    /// clause 6.3.17 warning on the response the handler renders.
    pub(crate) failures: Vec<LinkedResolutionFailure>,
    /// The projection an inline rendering walks for its sub-projections.
    projection: Option<&'a Projection>,
    /// What the join mode's clause 4.21 preparation left for the renderer.
    prepared: PreparedJoinProjection,
}

impl<'a> ResolvedJoin<'a> {
    /// Traverse to the linked entities and prepare the nested projections.
    ///
    /// A request that asked for no join answers `None`: `@none` and an absent
    /// `join` retrieve no linked entity at all (clause 4.5.23), so nothing is
    /// traversed rather than a traversal being paid for and discarded.
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
    ) -> Result<Option<ResolvedJoin<'a>>, EntityError> {
        let Some(join) = RetrievingJoin::from_mode(request.join) else {
            return Ok(None);
        };
        let (join_level, contained_by) = traversal_bounds(request.join_level, request.contained_by);

        let ResolvedLinkedEntities {
            entities: mut linked,
            failures,
        } = service
            .resolve_linked_entities(target, roots, join_level, request.sys_attrs, context, &contained_by)
            .await?;

        // Clause 4.21: a `pick`/`omit` term may carry a nested sub-projection
        // governing how the entities it links to render.
        let prepared = match request.projection {
            Some(projection) => service.prepare_join_projection(roots, &mut linked, projection, context, join).await?,
            None => PreparedJoinProjection::Unrestricted,
        };

        Ok(Some(ResolvedJoin {
            join,
            linked,
            failures,
            projection: request.projection,
            prepared,
        }))
    }

    /// The join an entity query asked for, resolved.
    ///
    /// Both query bindings describe their traversal with the very same
    /// parameter set, so neither spells the request out itself.
    ///
    /// # Errors
    ///
    /// Returns [`EntityError`] when the store cannot be read or a nested
    /// projection cannot be matched against the entities' attribute keys.
    pub(crate) async fn of_query(
        service: &EntityQueryService,
        target: &ContextTarget,
        roots: &[Entity],
        context: &ContextSpec,
        query: &'a EntityQueryParameters,
        sys_attrs: SysAttrsInclusion,
        projection: Option<&'a Projection>,
    ) -> Result<Option<ResolvedJoin<'a>>, EntityError> {
        let request = JoinRequest {
            join: query.join,
            join_level: query.join_level,
            contained_by: query.contained_by.as_deref(),
            sys_attrs,
            projection,
        };
        ResolvedJoin::prepare(service, target, roots, context, &request).await
    }

    /// The nested sub-projections an inline rendering applies, if the join mode
    /// left any for the renderer.
    pub(crate) fn linked_projection(&self) -> Option<LinkedProjection<'_>> {
        let projection = self.projection?;
        match &self.prepared {
            PreparedJoinProjection::Inline(index) => Some(LinkedProjection { projection, index }),
            PreparedJoinProjection::Unrestricted | PreparedJoinProjection::Flat(_) => None,
        }
    }

    /// Each linked entity paired with the clause 4.21 structural rule that
    /// governs it in a flat rendering.
    ///
    /// Clause 5.7.1.5 applies a restrictive list "to all Entities in the
    /// payload in the case of Linked Entity retrieval", and `id`, `type` and
    /// `scope` are members of the rendered representation rather than of the
    /// typed entity, so the terms that narrowed each entity's attributes have
    /// to reach the renderer to narrow these three as well. An entity no
    /// nested sub-projection reached is paired with `None` and renders whole.
    pub(crate) fn flat_linked(&self) -> impl Iterator<Item = (&Entity, Option<StructuralProjection<'_>>)> {
        let applied = match &self.prepared {
            PreparedJoinProjection::Flat(applied) => Some(applied),
            PreparedJoinProjection::Unrestricted | PreparedJoinProjection::Inline(_) => None,
        };
        self.linked
            .iter()
            .map(move |(id, entity)| (entity, applied.and_then(|applied| applied.get(id)).map(StructuralProjection::from)))
    }
}
