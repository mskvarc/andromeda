//! Resolving a clause 4.5.23 join for a read response.
//!
//! All three read endpoints -- `GET /entities`, `GET /entities/{id}` and
//! `POST /entityOperations/query` -- run the Linked Entity engine the same way
//! before they render: traverse from the roots through the Core source, give
//! every node the names the representation uses, index those names, and ask
//! the engine for the plan the writer renders. None of the handlers decides
//! anything about Linked Entities itself; they describe the join with a
//! [`JoinRequest`] and hand the plan to the writer.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    error::EntityError,
    query::{EntityQueryService, core_linked_source::CoreLinkedSource},
};
use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_linked_retrieval::{
    graph::{LinkedGraph, RootEdges},
    join::{RetrievingJoin, TraversalBounds},
    narrowing::{list::ListInForce, member::MemberRule},
    plan::JoinPlan,
    representation::LinkedRepresentation,
    traversal::traverse,
    typed::TypedEntities,
};
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::projection::attribute_iri_index::AttributeIriIndex;
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, linked_resolution_failure::LinkedResolutionFailure, sys_attrs::SysAttrsInclusion};
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
use std::num::NonZeroU32;

/// The clause 4.5.23 traversal a read request asked for.
///
/// Every read endpoint spells these out with its own parameter type;
/// gathering them here is what lets the join itself be run once.
pub(crate) struct JoinRequest<'a> {
    /// How the linked entities reach the response, as the request wrote it:
    /// `@none` and an absent `join` ask for no traversal at all.
    pub(crate) join: Option<JoinMode>,
    /// How many links deep the traversal goes; absent means the clause 4.5.23
    /// default depth.
    pub(crate) join_level: Option<NonZeroU32>,
    /// The entities the client already holds, which the traversal never
    /// retrieves again once a `joinLevel` is present.
    pub(crate) contained_by: Option<&'a [Identifier]>,
    /// Whether the linked entities carry the clause 4.8 system timestamps.
    pub(crate) sys_attrs: SysAttrsInclusion,
    /// The clause 4.21 list the request narrows its roots by, which reaches
    /// every Linked Entity (clause 5.7.1.5).
    pub(crate) list: ListInForce<'a>,
    /// The Linked Entities the holders of the roots nested under them, read
    /// into the model: they stand for an entity the store does not hold.
    pub(crate) carried: &'a [ReceivedState],
}

impl<'a> JoinRequest<'a> {
    /// The join an entity query asked for.
    ///
    /// Both query bindings describe their traversal with the very same
    /// parameter set, so neither spells the request out itself. `carried` is
    /// what the roots' holders nested under them.
    pub(crate) fn of_query(
        query: &'a EntityQueryParameters,
        sys_attrs: SysAttrsInclusion,
        list: ListInForce<'a>,
        carried: &'a [ReceivedState],
    ) -> JoinRequest<'a> {
        JoinRequest {
            join: query.join,
            join_level: query.join_level,
            contained_by: query.contained_by.as_deref(),
            sys_attrs,
            list,
            carried,
        }
    }
}

/// A traversed join, ready to be indexed and planned.
pub(crate) struct ResolvedJoin<'a> {
    /// The mode the linked entities reach the response in.
    join: RetrievingJoin,
    /// What the traversal reached.
    pub(crate) graph: LinkedGraph<Entity>,
    /// The list the request narrows its roots by.
    list: ListInForce<'a>,
    /// The expansion of every attribute key the payload carries, once indexed.
    index: AttributeIriIndex,
}

impl<'a> ResolvedJoin<'a> {
    /// Traverse to the linked entities.
    ///
    /// A request that asked for no join answers `None`: `@none` and an absent
    /// `join` retrieve no linked entity at all (clause 4.5.23), so nothing is
    /// traversed rather than a traversal being paid for and discarded.
    ///
    /// # Errors
    ///
    /// Returns [`EntityError`] when the store cannot be read.
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
        let bounds = TraversalBounds::of(request.join_level, request.contained_by);
        let source = CoreLinkedSource::new(service, target, request.sys_attrs, context).with_carried(request.carried);
        let graph = traverse(&source, RootEdges::of::<TypedEntities>(roots), &bounds).await?;
        Ok(Some(ResolvedJoin {
            join,
            graph,
            list: request.list,
            index: AttributeIriIndex::default(),
        }))
    }

    /// Expand every attribute key the roots and the graph carry, in the names
    /// the representation about to be rendered uses.
    ///
    /// Runs after the names are settled, so the writer matches the keys it
    /// actually writes. A list that narrows nothing matches nothing, and needs
    /// no index.
    ///
    /// # Errors
    ///
    /// Returns [`EntityError`] when a key cannot be expanded through the
    /// context.
    pub(crate) async fn index_attributes(&mut self, service: &EntityQueryService, roots: &[Entity], context: &ContextSpec) -> Result<(), EntityError> {
        if self.list.is_unrestricted() {
            return Ok(());
        }
        let prepared = service.engine().prepare_term_context(context);
        let expander = ContextualExpander::new(service.engine(), &prepared);
        let keys = roots
            .iter()
            .flat_map(TypedEntities::attribute_keys)
            .chain(self.graph.attribute_keys::<TypedEntities>());
        self.index = AttributeIriIndex::of_keys(keys, &expander).await?;
        Ok(())
    }

    /// The Linked Entities the response carries, as the engine plans them.
    pub(crate) fn plan(&self) -> JoinPlan<'_, TypedEntities> {
        JoinPlan::new(self.join, &self.graph, MemberRule::new(self.list, &self.index))
    }

    /// Every Linked Entity the holders of `versions` nested, at any depth.
    pub(crate) fn carried_by(versions: &mut [ReceivedState]) -> Vec<ReceivedState> {
        let mut carried = Vec::new();
        let mut pending: Vec<ReceivedState> = versions.iter_mut().flat_map(|version| version.linked.drain(..)).collect();
        while let Some(mut version) = pending.pop() {
            pending.append(&mut version.linked);
            carried.push(version);
        }
        carried
    }

    /// The Linked Entities a join, if one ran, could not resolve. Each becomes
    /// a clause 6.3.17 warning on the response.
    pub(crate) fn failures_of<'r>(resolved: Option<&'r ResolvedJoin<'_>>) -> &'r [LinkedResolutionFailure] {
        resolved.map_or(&[], |resolved| &resolved.graph.failures)
    }
}
