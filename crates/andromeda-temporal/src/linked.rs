//! Linked Entity retrieval (clause 4.5.23) over the temporal evolution of
//! entities, behind the `temporal_linked_retrieval` extension.
//!
//! # An extension beyond ETSI GS CIM 009 v1.9.1
//!
//! Clauses 5.7.3.3 and 5.7.4.3 give the temporal operations no Linked Entity
//! retrieval input, and clauses 5.7.3.4, 5.7.4.4 and 5.14.5.4 refuse a
//! projection or filter that indicates one. With the extension off that is
//! exactly what the broker does, and nothing here runs. With it on, the
//! temporal operations run the one Linked Entity engine every Core read runs,
//! through [`TemporalLinkedSource`], so the traversal, the list in force, the
//! first-encounter rule and the nesting are the Core's, applied over the
//! request's window.

use crate::{
    compilation::compile_projection,
    error::TemporalError,
    linked::temporal_linked_source::TemporalLinkedSource,
    port::read_scope::ScopeReach,
    remote_linked::TemporalLinkedWindow,
    service::TemporalService,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_linked_retrieval::{
    graph::{LinkedGraph, RootEdges},
    join::{RetrievingJoin, TraversalBounds},
    narrowing::{list::ListInForce, member::MemberRule},
    plan::JoinPlan,
    representation::LinkedRepresentation,
    traversal::traverse,
};
use andromeda_ngsi_ld_model::temporal::temporal_body::TemporalBody;
use andromeda_ngsi_ld_query::projection::{attribute_iri_index::AttributeIriIndex, compiled::Projection, expansion::expand_attrs};
use andromeda_ngsi_ld_render::temporal::temporal_evolutions::TemporalEvolutions;
use andromeda_ngsi_ld_representation::linked_resolution_failure::LinkedResolutionFailure;
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use iri_rs::IriBuf;

/// A traversed temporal join, ready to be planned and written.
pub struct TemporalLinkedJoin {
    /// The mode the Linked Entities reach the payload in.
    join: RetrievingJoin,
    /// What the traversal reached.
    graph: LinkedGraph<TemporalBody>,
    /// The request's clause 4.21 projection, expanded.
    projection: Projection,
    /// The request's `attrs`, expanded: the `pick` synonym of Tables
    /// 6.18.3.2-1 and 6.19.3.1-1.
    attrs: Vec<IriBuf>,
    /// The expansion of every attribute key the payload carries.
    index: AttributeIriIndex,
}

impl TemporalLinkedJoin {
    /// The Linked Entities the payload carries, as the engine plans them.
    #[must_use]
    pub fn plan(&self) -> JoinPlan<'_, TemporalEvolutions> {
        JoinPlan::new(self.join, &self.graph, MemberRule::new(self.list(), &self.index))
    }

    /// The Linked Entities the traversal could not resolve, each a clause
    /// 6.3.17 warning on the response.
    #[must_use]
    pub fn failures(&self) -> &[LinkedResolutionFailure] {
        &self.graph.failures
    }

    /// The list the request narrows its roots by, which reaches every Linked
    /// Entity (clause 5.7.1.5).
    fn list(&self) -> ListInForce<'_> {
        ListInForce::of(Some(&self.projection), &self.attrs)
    }
}

impl TemporalService {
    /// Traverse from the compact `roots` to their Linked Entities over the
    /// window `params` selects, when `params` asks for a join that retrieves.
    ///
    /// A root holds every instance of its attributes whatever representation
    /// the response is written in, so the edges it names are read off it as it
    /// is. `@none` and an absent `join` retrieve nothing, and answer `None`
    /// without a traversal.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the window or the projection cannot be
    /// compiled, an attribute key cannot be expanded, or the base entity store
    /// cannot be read. A single Linked Entity whose history cannot be
    /// rendered, and a remote source that cannot answer, are reported rather
    /// than propagated.
    pub async fn join_linked_temporal(
        &self,
        target: &ContextTarget,
        roots: &[TemporalBody],
        params: &TemporalQueryParameters,
        context: &ContextSpec,
    ) -> Result<Option<TemporalLinkedJoin>, TemporalError> {
        let Some(join) = RetrievingJoin::from_mode(params.join) else {
            return Ok(None);
        };
        let window = TemporalLinkedWindow::of_request(params);
        let compiled = self.compile_temporal_query(&window.to_parameters(), context, self.language_targeting()).await?;
        let scope = self.reader.read_scope(target, ScopeReach::Every).await?;
        let source = TemporalLinkedSource::new(self, target, scope, &compiled, &window, context);
        let bounds = TraversalBounds::of(params.join_level, params.contained_by.as_deref());
        let graph = traverse(&source, RootEdges::of::<TemporalEvolutions>(roots), &bounds).await?;

        let prepared = self.engine.prepare_term_context(context);
        let expander = ContextualExpander::new(&self.engine, &prepared);
        let mut joined = TemporalLinkedJoin {
            join,
            graph,
            projection: compile_projection(params, &expander).await?,
            attrs: expand_attrs(params.attrs.as_deref().unwrap_or_default(), &expander).await?,
            index: AttributeIriIndex::default(),
        };
        // A list that narrows nothing matches nothing, and needs no index.
        if !joined.list().is_unrestricted() {
            let keys = roots
                .iter()
                .flat_map(TemporalEvolutions::attribute_keys)
                .chain(joined.graph.attribute_keys::<TemporalEvolutions>());
            joined.index = AttributeIriIndex::of_keys(keys, &expander).await?;
        }
        Ok(Some(joined))
    }
}

/// Where the temporal operations read their Linked Entities from.
mod temporal_linked_source;

#[cfg(test)]
mod tests;
