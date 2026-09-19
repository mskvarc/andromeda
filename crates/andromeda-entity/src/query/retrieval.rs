//! Reading entities: clause 5.6.3 retrieval and clause 5.7.2 query.

use crate::{
    compiled_cache::LimitCap,
    error::EntityError,
    expanded_member::unwrap_expanded_ref,
    query::{
        EntityQueryService,
        dataset_filter::filter_entity_by_dataset_id,
        projection_remainder::projection_remainder,
        pushdown_refusal::refuse_unnarrowable,
        residual::finish_page,
        spatial_prefilter::SpatialNarrowing,
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::{
    cursor::PageCursor,
    dataset::DatasetSelector,
    entity::compiled::CompiledEntityQuery,
    pagination::CountPolicy,
    projection::compiled::Projection,
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_request::{pagination_cursor::decode_cursor, query::entity::parameters::EntityQueryParameters};
use andromeda_telemetry::entity::{self, EntityOperation};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

/// What a clause 5.6.3 retrieval narrows the retrieved entity down to.
///
/// The four members are the request's selection parameters, applied in the
/// order the clauses stack: `attrs` (clause 5.7.2) picks the attributes,
/// `pick`/`omit` (clause 4.21) project what is left, `datasetId`
/// (clause 4.5.5) narrows the surviving instances, and `sysAttrs`
/// decides whether the clause 4.8 timestamps come along.
pub struct RetrievalNarrowing<'a> {
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
    /// exists, or when `attrs` selected none of the attributes it has, and
    /// otherwise whatever the store or the context expansion failed with.
    pub async fn get_inner(&self, target: &ContextTarget, id: &Uri, narrowing: &RetrievalNarrowing<'_>, context: &ContextSpec) -> Result<Entity, EntityError> {
        let timer = entity::start(EntityOperation::Retrieve);
        let result: Result<Entity, EntityError> = async {
            let Some(expanded_entity) = self.reader.get(target, id).await? else {
                return Err(EntityError::not_found(id));
            };

            let mut entity = self.compact_to_entity(expanded_entity, context, Some(id.as_str()), narrowing.sys_attrs).await?;

            if narrowing.sys_attrs == SysAttrsInclusion::Include
                && let Some(snapshot) = self.reader.get_sys_attrs(target, id).await?
            {
                entity = entity.with_sys_attrs(&snapshot);
            }

            if !narrowing.attrs.is_empty() {
                let mut wanted_iris = Vec::with_capacity(narrowing.attrs.len());
                for attr in narrowing.attrs {
                    wanted_iris.push(self.expanded_key(attr.as_str(), context).await?);
                }
                self.filter_entity_attrs(&mut entity, &wanted_iris, context).await?;

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
                self.project_entity(&mut entity, projection, context).await?;
                // Tables 6.4.3.2-1 and 5.2.14.1-1 make `attrs` "a synonym for
                // pick", so the clause 5.7.1.4 rule above is the same rule here,
                // and clause 5.7.1.5 reduces the Entity "down to only contain the
                // defined Entity members" -- an Entity left with none of them is an
                // Entity the retrieval cannot answer with.
                if projection_remainder(&entity, projection).is_nothing() {
                    return Err(EntityError::not_found(id));
                }
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
            // Gated on the narrowed selection, not the one the client wrote: the
            // spatial index *applies* a geo relation rather than declining it, so a
            // store that cannot narrow `georel` is never asked to when the index
            // already answered it.
            let narrowed = match self.spatial_prefilter(target, &compiled) {
                SpatialNarrowing::Query(query) => query,
                SpatialNarrowing::EmptyResult => {
                    // A warm index that matched nothing is an empty page: returning
                    // it directly stops an empty candidate set from widening into an
                    // unconstrained store query (clause 4.10).
                    let total_count = match compiled.pagination.count {
                        CountPolicy::Emit => Some(0),
                        CountPolicy::Omit => None,
                    };
                    return Ok((Vec::new(), total_count));
                }
            };
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
                let ids: Vec<Uri> = page
                    .rows
                    .iter()
                    .filter_map(|row| {
                        unwrap_expanded_ref(row)
                            .as_object()
                            .and_then(|obj| obj.get("@id"))
                            .and_then(Value::as_str)
                            .and_then(|id| id.parse::<Uri>().ok())
                    })
                    .collect();
                self.reader.get_sys_attrs_batch(target, &ids).await?
            } else {
                HashMap::new()
            };

            let mut entities = Vec::with_capacity(page.rows.len());
            for value in page.rows {
                let id = unwrap_expanded_ref(&value)
                    .as_object()
                    .and_then(|obj| obj.get("@id"))
                    .and_then(Value::as_str)
                    .and_then(|id| id.parse::<Uri>().ok());

                let mut entity = self.compact_to_entity(value, context, None, sys_attrs).await?;

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
    pub async fn query_ids(&self, target: &ContextTarget, compiled: &CompiledEntityQuery) -> Result<Vec<Uri>, EntityError> {
        refuse_unnarrowable(*self.query_store.profile(), &compiled.selection)?;
        let outcome = self.query_store.query_ids(target, compiled).await?;
        self.finish_ids(target, outcome, compiled.pagination).await
    }
}
