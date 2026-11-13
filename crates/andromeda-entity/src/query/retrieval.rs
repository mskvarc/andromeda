//! Reading entities: clause 5.6.3 retrieval and clause 5.7.2 query.

use crate::{
    entity_type_match::entity_satisfies,
    error::EntityError,
    port::{retrieved_entity::RetrievedEntity, stored_entry::StoredEntry},
    query::{
        EntityQueryService,
        entity_versions::held_version,
        projection_remainder::state_remainder,
        pushdown_refusal::refuse_unnarrowable,
        residual::finish_page,
        state_projection::{keeps_a_standing_attribute, narrow_state},
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_linked_retrieval::narrowing::list::ListInForce;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    dataset::DatasetSelector,
    entity::compiled::CompiledEntityQuery,
    filter::language_targeting::LanguageTargeting,
    page_window::PageWindow,
    projection::{compiled::Projection, returned_attributes::ReturnedAttributes},
    type_selection::{expansion::expand_type_expression, grammar::TypeExprRaw},
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
use andromeda_telemetry::entity::{self, EntityOperation};
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use std::sync::Arc;

/// What a clause 5.6.3 retrieval narrows the retrieved entity down to.
///
/// The members are the request's selection parameters, applied in the order
/// the clauses stack: `type` (clause 5.7.1.4) decides whether the stored entity
/// is the target at all, `attrs` (clause 5.7.2) picks the attributes,
/// `pick`/`omit` (clause 4.21) project what is left, `datasetId`
/// (clause 4.5.5) narrows the surviving instances, and `sysAttrs`
/// decides whether the clause 4.8 timestamps come along.
pub struct RetrievalNarrowing<'a> {
    /// The clause 4.17 selector the entity's types must satisfy, absent when
    /// the request named no `type` (Table 6.5.3.1-1).
    pub entity_type: Option<&'a TypeExprRaw>,
    /// The attributes the response is narrowed to; empty selects all.
    pub attrs: &'a [AttributeName],
    /// Whether the clause 4.8 system timestamps are included.
    pub sys_attrs: SysAttrsInclusion,
    /// The attribute instances the response is narrowed to; empty selects all.
    pub dataset_ids: &'a [DatasetSelector],
    /// The clause 4.21 projection, absent when the request named neither
    /// `pick` nor `omit`.
    pub projection: Option<&'a Projection>,
}

impl EntityQueryService {
    /// Read the entity this broker holds for a single-entity retrieval
    /// (clause 5.6.3), narrowed to what the request selected, as the version
    /// a distributed retrieval merges what its sources answer onto.
    ///
    /// # Errors
    ///
    /// Returns [`EntityError::not_found`] when no entity with that identifier
    /// exists or it is past its `expiresAt` (clause 4.22), when its types fall
    /// outside the requested type selector, or when
    /// `attrs` or the clause 4.21 projection selected none of the members it
    /// has, and otherwise whatever the store or the context expansion failed
    /// with.
    pub async fn read_held(
        &self,
        target: &ContextTarget,
        id: &Identifier,
        narrowing: &RetrievalNarrowing<'_>,
        context: &ContextSpec,
    ) -> Result<ReceivedState, EntityError> {
        let timer = entity::start(EntityOperation::Retrieve);
        let result: Result<ReceivedState, EntityError> = async {
            let wanted_iris = self.expand_attribute_names(narrowing.attrs, context).await?;
            let unprojected = Projection::default();
            let returned = ReturnedAttributes::of(narrowing.projection.unwrap_or(&unprojected), &wanted_iris);
            // Clause 4.8: the entity-level system times sit on the stored row
            // beside the state, so they are read with it rather than asked for
            // in a second read.
            let Some(RetrievedEntity { state, sys_attrs }) = self.reader.get_with_sys_attrs(target, id, &returned).await? else {
                return Err(EntityError::not_found(id));
            };
            let now = Utc::now();
            // Clause 4.22: an Entity past its `expiresAt` is invalid, though
            // the expiry sweep has yet to delete it.
            if state.expired_at(now) {
                return Err(EntityError::not_found(id));
            }
            // Clause 5.7.1.4: the target is the entity "whose id (URI), and
            // where specified type, is equivalent", so an entity outside the
            // selector is not held locally for this retrieval.
            if let Some(raw) = narrowing.entity_type {
                let prepared = self.engine().prepare_term_context(context);
                let selector = expand_type_expression(raw, &ContextualExpander::new(self.engine(), &prepared)).await?;
                if !entity_satisfies(&state, &selector) {
                    return Err(EntityError::not_found(id));
                }
            }

            let state = narrow_to_selection(state, id, narrowing, &wanted_iris, now)?;
            // Tables 6.4.3.2-1 and 5.2.14.1-1 make `attrs` "a synonym for
            // pick", so the clause 5.7.1.4 rule `attrs` obeys is the same rule here,
            // and clause 5.7.1.5 reduces the Entity "down to only contain the
            // defined Entity members", an Entity left with none of them is an
            // Entity the retrieval cannot answer with.
            if let Some(projection) = narrowing.projection
                && state_remainder(&state, projection, now).is_nothing()
            {
                return Err(EntityError::not_found(id));
            }
            Ok(held_version(RetrievedEntity { state, sys_attrs }))
        }
        .await;
        timer.finish_result(&result);
        result
    }

    /// Select the page `window` selects of a parameterized entity query
    /// (clause 5.7.2) as the stored states it matched, each narrowed to the query's `attrs` and
    /// clause 4.21 projection and read with its entity-level clause 4.8 system
    /// times, with the optional clause 6.3.13 count. The query's target
    /// languages are admitted and select languageMap keys under
    /// `language_targeting`.
    ///
    /// The states are what [`EntityQueryService::compact_versions`] writes:
    /// no expiry (clause 4.22), `datasetId` (clause 4.5.5) or system-time
    /// (clause 4.8) rule of the rendering has been applied to them yet. A
    /// matching entity whose stored rows no longer read back keeps its place
    /// in the page as [`StoredEntry::Unreadable`], so the caller decides
    /// whether the page continues past it and how to report it.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::JsonLd`] when the request cannot be expanded
    /// against `context`, [`EntityError::Query`] when the request's filter
    /// expressions are not well formed, [`EntityError::OperationNotSupported`]
    /// when the store cannot narrow a predicate at all (clause 5.5.2), and
    /// [`EntityError::Persistence`] when the store could not answer the query.
    pub async fn query_states(
        &self,
        target: &ContextTarget,
        query: &EntityQueryParameters,
        window: &PageWindow,
        count: CountPolicy,
        context: &ContextSpec,
        language_targeting: LanguageTargeting,
    ) -> Result<(Vec<StoredEntry<RetrievedEntity>>, Option<i64>), EntityError> {
        let timer = entity::start(EntityOperation::Query);
        let result: Result<(Vec<StoredEntry<RetrievedEntity>>, Option<i64>), EntityError> = async {
            let compiled = self.compile_query_cached(query, context, language_targeting).await?;
            // The spatial index narrows only the default `location` instances;
            // the store still evaluates the geo-query, so the refusal below reads
            // the selection the store is actually handed.
            let narrowed = self.spatial_prefilter(target, &compiled).await;
            refuse_unnarrowable(*self.query_store.profile(), &narrowed.selection)?;
            let outcome = self.query_store.query_compiled(target, &narrowed, window, count).await?;
            let mut page = finish_page(outcome, window, count)?;
            for row in &mut page.rows {
                if let StoredEntry::Read(row) = row {
                    // The store decoded each state into its own allocation, so
                    // this borrows it mutably without a copy.
                    narrow_to_query(Arc::make_mut(&mut row.state), &compiled);
                }
            }
            Ok((page.rows, page.total_count))
        }
        .await;
        timer.finish_result(&result);
        result
    }

    /// Execute a compiled query returning only the identifiers of the matching
    /// entities `window` selects (clause 5.2.39 entity maps, and the
    /// operations that act on every match).
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::OperationNotSupported`] when the store cannot
    /// narrow a predicate of `compiled` at all (clause 5.5.2), and
    /// [`EntityError::Persistence`] when the store could not answer the query.
    pub async fn query_ids(&self, target: &ContextTarget, compiled: &CompiledEntityQuery, window: &PageWindow) -> Result<Vec<Identifier>, EntityError> {
        refuse_unnarrowable(*self.query_store.profile(), &compiled.selection)?;
        let outcome = self.query_store.query_ids(target, compiled, window).await?;
        self.finish_ids(target, outcome, window).await
    }
}

/// Apply a query's `attrs` (clause 5.7.2) and clause 4.21 projection to one
/// stored state before it is compacted.
///
/// Clause 5.7.2.5 reduces "every Entity within the payload body" to the
/// members the projection defines; an entity it would reduce to none of them
/// was never selected (`EntitySelection::projected_members`), so every entity
/// here keeps one and the page stays the size the store cut it to.
fn narrow_to_query(state: &mut EntityState, query: &CompiledEntityQuery) {
    narrow_state(state, ListInForce::of(Some(&query.projection), &query.selection.attrs));
}

/// Narrow a retrieved state to the `attrs` list (clause 5.7.2), expanded to
/// `wanted_iris`, and the clause 4.21 projection the request selected, before
/// it is compacted.
///
/// # Errors
///
/// Returns [`EntityError::not_found`] when `attrs` leaves the entity with no
/// attribute standing at `now`.
fn narrow_to_selection(
    state: Arc<EntityState>,
    id: &Identifier,
    narrowing: &RetrievalNarrowing<'_>,
    wanted_iris: &[IriBuf],
    now: DateTime<Utc>,
) -> Result<Arc<EntityState>, EntityError> {
    if narrowing.attrs.is_empty() && narrowing.projection.is_none() {
        return Ok(state);
    }
    let mut narrowed = Arc::unwrap_or_clone(state);
    // Clause 4.21 is applied before the clause 4.5.23 traversal reads the
    // entity's Relationships, matching the query path, where the projection
    // runs inside the query and the join follows it.
    narrow_state(&mut narrowed, ListInForce::of(narrowing.projection, wanted_iris));

    // Clause 5.7.1.4: "If the optional Attribute list is present and the
    // NGSI-LD endpoint does know about a matching Entity for the Entity
    // ID, but this Entity does not have any of the Attributes in the
    // Attribute list, then an error of type ResourceNotFound shall be
    // raised." An attribute whose every instance expired is not one the
    // entity has (clause 4.22).
    if !wanted_iris.is_empty() && !keeps_a_standing_attribute(&narrowed, now) {
        return Err(EntityError::not_found(id));
    }
    Ok(Arc::new(narrowed))
}
