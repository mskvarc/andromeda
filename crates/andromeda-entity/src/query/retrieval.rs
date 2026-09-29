//! Reading entities: clause 5.6.3 retrieval and clause 5.7.2 query.

use crate::{
    compiled_cache::LimitCap,
    entity_type_match::entity_satisfies,
    error::EntityError,
    expanded_member::unwrap_expanded_ref,
    query::{
        EntityQueryService,
        attribute_expansion::AttributeExpansions,
        attribute_projection::{filter_entity_attrs, project_entity},
        dataset_filter::filter_entity_by_dataset_id,
        projection_remainder::projection_remainder,
        pushdown_refusal::refuse_unnarrowable,
        residual::finish_page,
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::{
    cursor::PageCursor,
    dataset::DatasetSelector,
    entity::compiled::CompiledEntityQuery,
    projection::compiled::Projection,
    term_expander::TermExpander,
    type_selection::{expansion::expand_type_expression, grammar::TypeExprRaw},
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_request::{pagination_cursor::decode_cursor, query::entity::parameters::EntityQueryParameters};
use andromeda_telemetry::entity::{self, EntityOperation};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

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
    /// Retrieve a single entity by identifier (clause 5.6.3), narrowed to what
    /// the request selected.
    ///
    /// # Errors
    ///
    /// Returns [`EntityError::not_found`] when no entity with that identifier
    /// exists, when its types fall outside the requested type selector, or when
    /// `attrs` selected none of the attributes it has, and
    /// otherwise whatever the store or the context expansion failed with.
    pub async fn get_inner(
        &self,
        target: &ContextTarget,
        id: &Identifier,
        narrowing: &RetrievalNarrowing<'_>,
        context: &ContextSpec,
    ) -> Result<Entity, EntityError> {
        let timer = entity::start(EntityOperation::Retrieve);
        let result: Result<Entity, EntityError> = async {
            let Some(expanded_entity) = self.reader.get(target, id).await? else {
                return Err(EntityError::not_found(id));
            };
            // Clause 5.7.1.4: the target is the entity "whose id (URI), and
            // where specified type, is equivalent", so an entity outside the
            // selector is not held locally for this retrieval.
            if let Some(raw) = narrowing.entity_type {
                let prepared = self.engine().prepare_term_context(context);
                let selector = expand_type_expression(raw, &ContextualExpander::new(self.engine(), &prepared)).await?;
                if !entity_satisfies(&expanded_entity, &selector) {
                    return Err(EntityError::not_found(id));
                }
            }

            let mut entity = self
                .compact_to_entity(expanded_entity, &self.engine().compactor(context), Some(id.as_str()), narrowing.sys_attrs)
                .await?;

            if narrowing.sys_attrs == SysAttrsInclusion::Include
                && let Some(snapshot) = self.reader.get_sys_attrs(target, id).await?
            {
                entity = entity.with_sys_attrs(&snapshot);
            }

            if !narrowing.attrs.is_empty() || narrowing.projection.is_some() {
                self.narrow_to_selection(&mut entity, id, narrowing, context).await?;
            }

            if !narrowing.dataset_ids.is_empty() {
                filter_entity_by_dataset_id(&mut entity, narrowing.dataset_ids);
            }

            Ok(entity)
        }
        .await;
        timer.finish_result(&result);
        result
    }

    /// Narrow a retrieved entity to the `attrs` list (clause 5.7.2) and the
    /// clause 4.21 projection the request selected.
    ///
    /// The context is prepared once for both, and each member name expanded
    /// once for both.
    ///
    /// # Errors
    ///
    /// Returns [`EntityError::not_found`] when the selection leaves the entity
    /// with nothing to answer with, and whatever the context expansion failed
    /// with otherwise.
    async fn narrow_to_selection(
        &self,
        entity: &mut Entity,
        id: &Identifier,
        narrowing: &RetrievalNarrowing<'_>,
        context: &ContextSpec,
    ) -> Result<(), EntityError> {
        let prepared = self.engine().prepare_term_context(context);
        let mut expansions = AttributeExpansions::new(self.engine(), &prepared);
        if !narrowing.attrs.is_empty() {
            let expander = ContextualExpander::new(self.engine(), &prepared);
            let mut wanted_iris = Vec::with_capacity(narrowing.attrs.len());
            for attr in narrowing.attrs {
                wanted_iris.push(expander.expand_term(attr.as_str()).await?);
            }
            filter_entity_attrs(entity, &wanted_iris, &mut expansions).await?;

            // Clause 5.7.1.4: "If the optional Attribute list is present and
            // the NGSI-LD endpoint does know about a matching Entity for the
            // Entity ID, but this Entity does not have any of the Attributes in
            // the Attribute list, then an error of type ResourceNotFound shall
            // be raised."
            if !entity.has_attribute_members() {
                return Err(EntityError::not_found(id));
            }
        }

        // Clause 4.21 is applied before the clause 4.5.23 traversal reads the
        // entity's Relationships, matching the query path, where the projection
        // runs inside the query and the join follows it.
        if let Some(projection) = narrowing.projection {
            project_entity(entity, projection, &mut expansions).await?;
            // Tables 6.4.3.2-1 and 5.2.14.1-1 make `attrs` "a synonym for
            // pick", so the clause 5.7.1.4 rule above is the same rule here,
            // and clause 5.7.1.5 reduces the Entity "down to only contain the
            // defined Entity members" -- an Entity left with none of them is an
            // Entity the retrieval cannot answer with.
            if projection_remainder(entity, projection).is_nothing() {
                return Err(EntityError::not_found(id));
            }
        }
        Ok(())
    }

    /// Execute a parameterized entity query (clause 5.7.2), returning matching
    /// entities and the optional clause 6.3.13 count.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::JsonLd`] when the request or a result cannot be
    /// expanded or compacted against `context`, [`EntityError::Query`] when the
    /// request's filter expressions are not well formed,
    /// [`EntityError::OperationNotSupported`] when the store cannot narrow a
    /// predicate at all (clause 5.5.2), and [`EntityError::Persistence`] when
    /// the store could not answer the query.
    pub async fn query(&self, target: &ContextTarget, query: &EntityQueryParameters, context: &ContextSpec) -> Result<(Vec<Entity>, Option<i64>), EntityError> {
        let timer = entity::start(EntityOperation::Query);
        let result: Result<(Vec<Entity>, Option<i64>), EntityError> = async {
            let compiled = self.compile_query_cached(query, context, LimitCap::Capped).await?;
            // The opaque pagination cursor (clause 5.5.9.1) is a per-request
            // position that never took part in compilation, so it is decoded and
            // set on an owned copy here rather than being keyed into the compiled
            // cache. A request without one keeps sharing the cached compilation.
            let compiled = match decode_cursor(query.cursor.as_deref())? {
                None => compiled,
                Some(cursor) => {
                    let mut owned = Arc::unwrap_or_clone(compiled);
                    owned.keyset = Some(cursor);
                    Arc::new(owned)
                }
            };
            // The spatial index narrows only the default `location` instances;
            // the store still evaluates the geo-query, so the refusal below reads
            // the selection the store is actually handed.
            let narrowed = self.spatial_prefilter(target, &compiled).await;
            refuse_unnarrowable(*self.query_store.profile(), &narrowed.selection)?;
            let outcome = self.query_store.query_compiled(target, &narrowed).await?;
            let mut page = finish_page(outcome, compiled.pagination)?;
            // Clause 5.5.9.1: a backward or from-end keyset page was read
            // descending so a single index scan could serve it; it is reversed
            // here to the ascending order the client sees on every page.
            if matches!(compiled.keyset, Some(PageCursor::Before(_) | PageCursor::End)) {
                page.rows.reverse();
            }
            let sys_attrs = query.parse_options()?.sys_attrs;

            // One batch read rather than one per entity.
            let sys_attrs_map = if sys_attrs == SysAttrsInclusion::Include {
                let ids: Vec<Identifier> = page
                    .rows
                    .iter()
                    .filter_map(|row| {
                        unwrap_expanded_ref(row)
                            .as_object()
                            .and_then(|obj| obj.get("@id"))
                            .and_then(Value::as_str)
                            .and_then(|id| id.parse::<Identifier>().ok())
                    })
                    .collect();
                self.reader.get_sys_attrs_batch(target, &ids).await?
            } else {
                HashMap::new()
            };

            let compactor = self.engine().compactor(context);
            let mut entities = Vec::with_capacity(page.rows.len());
            for value in page.rows {
                let id = unwrap_expanded_ref(&value)
                    .as_object()
                    .and_then(|obj| obj.get("@id"))
                    .and_then(Value::as_str)
                    .and_then(|id| id.parse::<Identifier>().ok());

                let mut entity = self.compact_to_entity(value, &compactor, None, sys_attrs).await?;

                if sys_attrs == SysAttrsInclusion::Include
                    && let Some(ref id) = id
                    && let Some(snapshot) = sys_attrs_map.get(id)
                {
                    entity = entity.with_sys_attrs(snapshot);
                }

                entities.push(entity);
            }

            let mut entities = self.apply_projection(entities, &compiled, context).await?;

            if let Some(dataset_ids) = query.dataset_id.as_deref() {
                for entity in &mut entities {
                    filter_entity_by_dataset_id(entity, dataset_ids);
                }
            }

            Ok((entities, page.total_count))
        }
        .await;
        timer.finish_result(&result);
        result
    }

    /// Execute a compiled query returning only entity identifiers
    /// (clause 5.2.39 entity maps).
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::OperationNotSupported`] when the store cannot
    /// narrow a predicate of `compiled` at all (clause 5.5.2), and
    /// [`EntityError::Persistence`] when the store could not answer the query.
    pub async fn query_ids(&self, target: &ContextTarget, compiled: &CompiledEntityQuery) -> Result<Vec<Identifier>, EntityError> {
        refuse_unnarrowable(*self.query_store.profile(), &compiled.selection)?;
        let outcome = self.query_store.query_ids(target, compiled).await?;
        self.finish_ids(target, outcome, compiled.pagination).await
    }
}
