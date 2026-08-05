use crate::{
    core::error::AndromedaError,
    http::parameter::query::{SharedQueryParams, entity::EntityQueryParameters},
    jsonld::{CompactOptions, ContextSpec, JsonLdEngine, term_expansion::ContextualExpander},
    repository::{attribute_names::AttributeNameCache, entity::repo::EntityRepository},
    service::{
        csource_forwarding::{ForwardedEntities, merge_query_results},
        entity::compiled_cache::{CompiledQueryCache, CompiledQueryKey, LimitCap},
        spatial::{SpatialKey, entity_index::SpatialEntityIndex, flatten_or_type_iris},
    },
    tenancy::types::TenantRef,
    worker::attribute_current_materializer::AttributeCurrentBarrier,
};
use andromeda_domain::{
    attribute::{Attribute, AttributeValue, name::AttributeName},
    entity::Entity,
    sys_attrs::SysAttrsInclusion,
    uri::Uri,
};
use andromeda_geometry::geometry::Geometry;
use andromeda_query::{
    entity::{compiled::CompiledEntityQuery, selection::EntitySelection},
    error::QueryError,
    filter::{
        expansion::{apply_expand_values, apply_json_keys, expand_q_expression},
        path::expanded_attribute_name,
        sql::{attribute_names::ResolvedAttributeNames, projection::referenced_attribute_names},
    },
    geo::{compile as geo_compile, query::GeoQuery},
    identifier::parse_id_list,
    ordering::{
        collation::IcuCollation,
        compiled::ResultOrdering,
        distance::OrderingGeometry,
        term::{CompiledOrderTerm, OrderTarget, RawOrderTarget},
    },
    pagination::{CountPolicy, Pagination},
    pattern::Pattern,
    projection::{
        compiled::Projection,
        expansion::{expand_attrs, expand_projection_terms},
    },
    scope::query::ScopeQuery,
    term_expander::TermExpander,
    type_selection::{expansion::expand_type_expression, parser as type_parser},
};
use iri_rs::IriBuf;
use serde_json::Value;
use sqlx::postgres::PgPool;
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::Arc,
};
use urn_rs::Urn;

/// Service for querying and retrieving NGSI-LD entities.
pub struct EntityQueryService {
    repo: EntityRepository,
    engine: JsonLdEngine,
    compiled_cache: CompiledQueryCache,
    spatial: Arc<SpatialEntityIndex>,
    tenant: TenantRef,
    attribute_current: AttributeCurrentBarrier,
    attribute_names: Arc<AttributeNameCache>,
}

type ResolveLinkedResult = Result<HashMap<String, Entity>, AndromedaError>;
type ResolveLinkedFuture<'a> = Pin<Box<dyn Future<Output = ResolveLinkedResult> + Send + 'a>>;

impl EntityQueryService {
    /// Create a new entity query service backed by the given connection pool.
    pub fn new(
        pool: &PgPool,
        engine: JsonLdEngine,
        spatial: Arc<SpatialEntityIndex>,
        tenant: TenantRef,
        attribute_current: AttributeCurrentBarrier,
        attribute_names: Arc<AttributeNameCache>,
    ) -> EntityQueryService {
        let repo = EntityRepository::new(pool);
        EntityQueryService {
            repo,
            engine,
            compiled_cache: CompiledQueryCache::new(),
            spatial,
            tenant,
            attribute_current,
            attribute_names,
        }
    }

    /// Resolve the attribute IRIs a compiled `q` filter references, and wait
    /// for the `attribute_current` projection to catch up if any of them will
    /// be answered from it.
    ///
    /// Resolution happens here, per execution, rather than at compile time:
    /// compiled queries are cached for minutes, and an attribute IRI that was
    /// unknown when the filter was first compiled must not stay pinned as
    /// unknown — its predicates would keep compiling to a constant `FALSE`
    /// long after the attribute started being written.
    ///
    /// The barrier is only taken when at least one IRI resolved. Nothing
    /// resolving means every projection-backed predicate is already a constant
    /// `FALSE`, so there is no pending write that could change the answer.
    async fn resolve_q_attributes(&self, compiled: &CompiledEntityQuery) -> Result<ResolvedAttributeNames, AndromedaError> {
        let Some(q_expr) = &compiled.selection.q_expr else {
            return Ok(ResolvedAttributeNames::default());
        };
        let mut names = Vec::new();
        referenced_attribute_names(q_expr, &mut names);
        let resolved = self.attribute_names.lookup(&names).await?;
        if !resolved.is_empty() {
            self.attribute_current.wait_for_pending_writes().await;
        }
        Ok(resolved)
    }

    /// Return a reference to the underlying JSON-LD engine.
    pub const fn engine(&self) -> &JsonLdEngine {
        &self.engine
    }

    async fn compact_to_entity(&self, expanded: Value, context: &ContextSpec, id_override: Option<&str>) -> Result<Entity, AndromedaError> {
        let compacted = self.engine.compact(expanded, context, CompactOptions::default()).await?;

        let mut obj = match compacted {
            Value::Array(mut arr) if !arr.is_empty() => match arr.swap_remove(0) {
                Value::Object(map) => Some(map),
                _other => None,
            },
            Value::Object(map) => Some(map),
            _other => None,
        }
        .ok_or_else(|| AndromedaError::entity_validation("Entity data is not a JSON object"))?;

        if let Some(t) = obj.remove("@type") {
            obj.insert("type".to_string(), t);
        }

        let id = id_override
            .map(|s| s.to_string())
            .or_else(|| obj.get("id").and_then(|v| v.as_str()).map(str::to_string))
            .or_else(|| obj.get("@id").and_then(|v| v.as_str()).map(str::to_string))
            .ok_or_else(|| AndromedaError::entity_validation("Entity missing id field"))?;

        obj.remove("@id");
        obj.insert("id".to_string(), serde_json::json!(id));

        let val = Value::Object(obj);

        let mut entity: Entity = serde_json::from_value(val).map_err(|err| AndromedaError::entity_validation(format!("Invalid entity data: {}", err)))?;

        // Filter out attribute instances whose expiresAt has passed (clause 4.22).
        // The background worker handles DB cleanup, but this ensures clients never
        // see stale expired instances between worker cycles.
        entity.remove_expired_attributes(chrono::Utc::now());

        Ok(entity)
    }

    // -------------------------------------------------------------------------
    // Read operations
    // -------------------------------------------------------------------------

    /// Retrieve a single entity by URN, optionally filtering attributes and dataset IDs.
    pub async fn get_inner(
        &self,
        id: &Urn,
        attrs: &[String],
        sys_attrs: SysAttrsInclusion,
        dataset_ids: &[String],
        context: &ContextSpec,
    ) -> Result<Entity, AndromedaError> {
        let id_str = id.to_string();
        let result = self.repo.get(&id_str).await?;

        if let Some(expanded_entity) = result {
            let mut entity = self.compact_to_entity(expanded_entity, context, Some(&id_str)).await?;

            if sys_attrs == SysAttrsInclusion::Include
                && let Some(sa) = self.repo.get_sys_attrs(&id_str).await?
            {
                entity = entity.with_sys_attrs(&sa);
            }

            if !attrs.is_empty() {
                let mut wanted_iris = Vec::new();
                for a in attrs {
                    wanted_iris.push(self.expanded_key(a, context).await?);
                }
                self.filter_entity_attrs(&mut entity, &wanted_iris, context).await?;

                if entity.attrs.is_empty() {
                    return Err(AndromedaError::entity_not_found(id_str));
                }
            }

            if !dataset_ids.is_empty() {
                filter_entity_by_dataset_id(&mut entity, dataset_ids);
            }

            return Ok(entity);
        }

        Err(AndromedaError::entity_not_found(id_str))
    }

    /// Execute a parameterized entity query, returning matching entities and an optional total count.
    pub async fn query(&self, query: &EntityQueryParameters, context: &ContextSpec) -> Result<(Vec<Entity>, Option<i64>), AndromedaError> {
        let compiled = self.compile_query_cached(query, context, LimitCap::Capped).await?;
        let compiled_for_repo = self.spatial_prefilter(&compiled);
        let attribute_names = self.resolve_q_attributes(&compiled_for_repo).await?;
        let query_result = self.repo.query_compiled(&compiled_for_repo, &attribute_names).await?;
        let sys_attrs = query.parse_options().sys_attrs;

        // Batch-fetch sys_attrs for all entities in one query instead of N+1
        let sys_attrs_map = if sys_attrs == SysAttrsInclusion::Include {
            let ids: Vec<String> = query_result
                .rows
                .iter()
                .filter_map(|v| {
                    let obj = match v {
                        serde_json::Value::Array(arr) => arr.first().and_then(|v| v.as_object()),
                        serde_json::Value::Object(obj) => Some(obj),
                        _other => None,
                    };
                    obj.and_then(|o| o.get("@id").and_then(|v| v.as_str()).map(String::from))
                })
                .collect();
            self.repo.get_sys_attrs_batch(&ids).await?
        } else {
            HashMap::new()
        };

        let mut entities = Vec::new();
        for value in query_result.rows {
            let id_str = super::unwrap_expanded_ref(&value)
                .as_object()
                .and_then(|o| o.get("@id").and_then(|v| v.as_str()).map(String::from));

            let mut entity = self.compact_to_entity(value, context, None).await?;

            if sys_attrs == SysAttrsInclusion::Include
                && let Some(ref id_str) = id_str
                && let Some(sa) = sys_attrs_map.get(id_str)
            {
                entity = entity.with_sys_attrs(sa);
            }

            entities.push(entity);
        }

        let mut entities = self.apply_projection(entities, &compiled, context).await?;

        let dataset_ids = query.parse_dataset_ids();
        if !dataset_ids.is_empty() {
            for entity in &mut entities {
                filter_entity_by_dataset_id(entity, &dataset_ids);
            }
        }

        Ok((entities, query_result.total_count))
    }

    /// Recursively resolve linked entities up to the given join level.
    ///
    /// The recursive work runs through [`Self::resolve_linked_inner`] on an
    /// owned `contained_by` set that is mutated in place as each level adds
    /// the ids it has just resolved; no per-level HashSet clone occurs and
    /// recursion walks `&Entity` references instead of cloning entities.
    pub fn resolve_linked_entities<'a>(
        &'a self,
        entities: &'a [Entity],
        join_level: u32,
        sys_attrs: SysAttrsInclusion,
        context: &'a ContextSpec,
        contained_by: &HashSet<String>,
    ) -> ResolveLinkedFuture<'a> {
        const MAX_JOIN_LEVEL: u32 = 5;
        let join_level = join_level.min(MAX_JOIN_LEVEL);
        let refs: Vec<&'a Entity> = entities.iter().collect();
        let owned = contained_by.clone();
        Box::pin(self.resolve_linked_inner(refs, join_level, sys_attrs, context, owned))
    }

    fn resolve_linked_inner<'a>(
        &'a self,
        entities: Vec<&'a Entity>,
        join_level: u32,
        sys_attrs: SysAttrsInclusion,
        context: &'a ContextSpec,
        mut contained_by: HashSet<String>,
    ) -> Pin<Box<dyn Future<Output = ResolveLinkedResult> + Send + 'a>> {
        Box::pin(async move {
            if join_level == 0 {
                return Ok(HashMap::new());
            }

            let mut linked_ids: HashSet<String> = HashSet::new();
            let entity_ids: HashSet<String> = entities.iter().map(|e| e.id.to_string()).collect();

            for entity in &entities {
                collect_relationship_objects(entity, &mut linked_ids);
            }
            linked_ids.retain(|id| !entity_ids.contains(id) && !contained_by.contains(id));

            let linked_id_vec: Vec<String> = linked_ids.into_iter().collect();
            let batch_payloads = self.repo.get_batch(&linked_id_vec).await?;

            let batch_sys_attrs = if sys_attrs == SysAttrsInclusion::Include {
                self.repo.get_sys_attrs_batch(&linked_id_vec).await?
            } else {
                HashMap::new()
            };

            let mut linked_map = HashMap::new();
            for (linked_id, expanded) in batch_payloads {
                match self.compact_to_entity(expanded, context, Some(&linked_id)).await {
                    Ok(mut entity) => {
                        if sys_attrs == SysAttrsInclusion::Include
                            && let Some(sa) = batch_sys_attrs.get(&linked_id)
                        {
                            entity = entity.with_sys_attrs(sa);
                        }
                        linked_map.insert(linked_id, entity);
                    }
                    Err(e) => {
                        tracing::debug!("Failed to compact linked entity {}: {}", linked_id, e);
                    }
                }
            }

            if join_level > 1 && !linked_map.is_empty() {
                contained_by.extend(entity_ids);
                contained_by.reserve(linked_map.len());
                for key in linked_map.keys() {
                    contained_by.insert(key.clone());
                }
                let level2_refs: Vec<&Entity> = linked_map.values().collect();
                let level2_linked = self.resolve_linked_inner(level2_refs, join_level - 1, sys_attrs, context, contained_by).await?;
                linked_map.extend(level2_linked);
            }

            Ok(linked_map)
        })
    }

    // -------------------------------------------------------------------------
    // ID-only query (for EntityMap creation)
    // -------------------------------------------------------------------------

    /// Compile an entity query with the default limit cap applied.
    pub async fn compile_query_public(&self, params: &EntityQueryParameters, context: &ContextSpec) -> Result<CompiledEntityQuery, AndromedaError> {
        let arc = self.compile_query_cached(params, context, LimitCap::Capped).await?;
        Ok(Arc::unwrap_or_clone(arc))
    }

    /// Compile an entity query without applying a limit cap.
    pub async fn compile_query_uncapped(&self, params: &EntityQueryParameters, context: &ContextSpec) -> Result<CompiledEntityQuery, AndromedaError> {
        let arc = self.compile_query_cached(params, context, LimitCap::Uncapped).await?;
        Ok(Arc::unwrap_or_clone(arc))
    }

    /// Cache-aware compilation entry point used by every caller.
    ///
    /// First consults the in-memory [`CompiledQueryCache`] keyed on the
    /// request parameters and the active JSON-LD context fingerprint; on miss,
    /// compiles and stores the result.
    async fn compile_query_cached(
        &self,
        params: &EntityQueryParameters,
        context: &ContextSpec,
        limit_cap: LimitCap,
    ) -> Result<Arc<CompiledEntityQuery>, AndromedaError> {
        let key = CompiledQueryKey::from_params(params, context, limit_cap);
        self.compiled_cache.get_or_compile(key, || self.compile_query(params, context, limit_cap)).await
    }

    /// Execute a compiled query returning only entity IDs.
    pub async fn query_ids(&self, compiled: &CompiledEntityQuery) -> Result<Vec<String>, AndromedaError> {
        let attribute_names = self.resolve_q_attributes(compiled).await?;
        self.repo.query_ids(compiled, &attribute_names).await
    }

    /// Merge local query results with forwarded context source results.
    ///
    /// The merge stays in the typed domain model throughout; forwarded entities
    /// were already parsed to [`Entity`] at the CSR boundary, so no `Value`
    /// round-trip occurs. Per spec 5.7.2, the optional `attrs_filter` is
    /// applied after merge (context sources return all attributes).
    pub fn merge_with_forwarded(local_entities: Vec<Entity>, forwarded: Vec<ForwardedEntities>, attrs_filter: Option<&str>) -> Vec<Entity> {
        let mut result = merge_query_results(local_entities, forwarded);
        if let Some(attrs_str) = attrs_filter {
            let requested: Vec<&str> = attrs_str.split(',').map(str::trim).collect();
            for entity in &mut result {
                entity.attrs.retain(|k, _| requested.iter().any(|a| k.ends_with(a)));
            }
            result.retain(|e| !e.attrs.is_empty());
        }
        result
    }

    // -------------------------------------------------------------------------
    // Query compilation (private)
    // -------------------------------------------------------------------------

    async fn compile_query(&self, params: &EntityQueryParameters, context: &ContextSpec, limit_cap: LimitCap) -> Result<CompiledEntityQuery, AndromedaError> {
        let ids = parse_id_list(&params.parse_ids().unwrap_or_default())?;
        let id_pattern = params
            .id_pattern
            .as_deref()
            .map(Pattern::parse)
            .transpose()
            .map_err(|e| AndromedaError::entity_validation(e.to_string()))?;

        let type_expr = if let Some(raw_types) = params.entity_type.as_deref() {
            let raw = type_parser::parse(raw_types)?;
            let expanded = expand_type_expression(&raw, &ContextualExpander::new(&self.engine, context)).await?;
            Some(expanded)
        } else {
            None
        };

        let attrs = expand_attrs(&params.parse_attrs(), &ContextualExpander::new(&self.engine, context)).await?;

        let q_expr = expand_q_expression(params.q.as_deref(), &ContextualExpander::new(&self.engine, context)).await?;

        // Apply expandValues: expand string RHS values for specified attributes (spec 6.4.3.2)
        let q_expr = if let Some(q_expr) = q_expr {
            if let Some(ref ev) = params.expand_values {
                let ev_attrs: Vec<String> = ev.split(',').map(|s| s.trim().to_string()).collect();
                let ev_iris = expand_attrs(&ev_attrs, &ContextualExpander::new(&self.engine, context)).await?;
                let ev_set = opted_in_attribute_names(&ev_iris)?;
                Some(apply_expand_values(q_expr, &ev_set, &ContextualExpander::new(&self.engine, context)).await?)
            } else {
                Some(q_expr)
            }
        } else {
            None
        };

        // Apply jsonKeys: expand sub-path keys for specified attributes (spec 6.4.3.2)
        let q_expr = if let Some(q_expr) = q_expr {
            if let Some(ref jk) = params.json_keys {
                let jk_attrs: Vec<String> = jk.split(',').map(|s| s.trim().to_string()).collect();
                let jk_iris = expand_attrs(&jk_attrs, &ContextualExpander::new(&self.engine, context)).await?;
                let jk_set = opted_in_attribute_names(&jk_iris)?;
                Some(apply_json_keys(q_expr, &jk_set, &ContextualExpander::new(&self.engine, context)).await?)
            } else {
                Some(q_expr)
            }
        } else {
            None
        };

        let geo = self.compile_geo_query(params, context).await?;
        let opts = params.parse_options();

        let pick = expand_projection_terms(&params.parse_pick(), &ContextualExpander::new(&self.engine, context)).await?;
        let omit = expand_projection_terms(&params.parse_omit(), &ContextualExpander::new(&self.engine, context)).await?;

        let order_by = {
            let raw = params.parse_order_by();
            let mut compiled = Vec::new();
            for term in &raw {
                let target = match RawOrderTarget::classify(&term.attr) {
                    RawOrderTarget::Id => OrderTarget::Id,
                    RawOrderTarget::Type => OrderTarget::Type,
                    RawOrderTarget::Attribute(name) => OrderTarget::Attribute(self.expanded_key(name, context).await?),
                };
                compiled.push(CompiledOrderTerm {
                    target,
                    direction: term.direction,
                    collation: params
                        .collation
                        .as_deref()
                        .map(IcuCollation::parse)
                        .transpose()
                        .map_err(|error| AndromedaError::entity_validation(error.to_string()))?,
                });
            }
            compiled
        };

        // Compile orderFrom/orderGeometry into OrderingGeometry (S5.2.43)
        let ordering_geometry = if let Some(ref order_from) = params.order_from {
            let coordinates: serde_json::Value = serde_json::from_str(order_from)
                .map_err(|_| AndromedaError::entity_validation(format!("Invalid orderFrom value: '{}' is not valid JSON", order_from)))?;
            let geometry_type = params.order_geometry.as_deref().unwrap_or("Point");
            let geojson_value = serde_json::json!({
                "type": geometry_type,
                "coordinates": coordinates
            });
            let geometry = Geometry::from_geojson_value(&geojson_value)
                .map_err(|e| AndromedaError::entity_validation(format!("Invalid orderFrom/orderGeometry combination: {}", e)))?;
            Some(OrderingGeometry::new(geometry))
        } else {
            None
        };

        // Validate: distance ordering requires a reference geometry (S5.7.2.4)
        let has_distance_term = order_by.iter().any(|t| t.direction.is_distance());
        if has_distance_term && geo.is_none() && ordering_geometry.is_none() {
            return Err(AndromedaError::entity_validation(
                "Distance-based ordering requires either a geo-query or orderFrom parameter".to_string(),
            ));
        }

        let pagination = Pagination {
            limit: match limit_cap {
                LimitCap::Capped => params.effective_limit(),
                LimitCap::Uncapped => params.effective_limit_uncapped(),
            },
            offset: params.effective_offset(),
            count: if opts.count { CountPolicy::Emit } else { CountPolicy::Omit },
        };

        let compiled = CompiledEntityQuery::builder()
            .selection(
                EntitySelection::builder()
                    .ids(ids)
                    .id_pattern(id_pattern)
                    .type_expr(type_expr)
                    .attrs(attrs)
                    .q_expr(q_expr)
                    .geo(geo)
                    .scope_q(params.scope_q.as_deref().map(ScopeQuery::parse).transpose()?)
                    .build(),
            )
            .projection(Projection::builder().pick(pick).omit(omit).build())
            .ordering(ResultOrdering::builder().terms(order_by).geometry(ordering_geometry).build())
            .pagination(pagination)
            .build();

        tracing::info!("Compiled query: {:?}", compiled);

        Ok(compiled)
    }

    /// Consult [`SpatialEntityIndex`] to narrow `compiled.selection.ids` before SQL,
    /// and drop `compiled.selection.geo` when the index was able to produce an
    /// authoritative candidate set.
    ///
    /// Falls through (returns the original `Arc` unchanged) when the query
    /// is not geo-accelerated: no `geo`, complex `type_expr` (`And`), cold
    /// partition, or a reference geometry the `geo` crate can't parse. SQL
    /// still runs with the full georel filter in those cases.
    fn spatial_prefilter(&self, compiled: &Arc<CompiledEntityQuery>) -> Arc<CompiledEntityQuery> {
        let Some(ref geo_q) = compiled.selection.geo else {
            return Arc::clone(compiled);
        };
        let Some(ref type_expr) = compiled.selection.type_expr else {
            return Arc::clone(compiled);
        };
        let Some(type_iris) = flatten_or_type_iris(type_expr) else {
            return Arc::clone(compiled);
        };
        // clone: the spatial key owns its IRI and outlives the borrowed query
        let geoproperty_iri = geo_q.geoproperty_iri.clone();
        let ref_geom: &geo::Geometry<f64> = geo_q.geometry.as_ref();

        let mut any_warm = false;
        let mut union: HashSet<Urn> = HashSet::new();
        for type_iri in type_iris {
            let key = SpatialKey::new(self.tenant.clone(), type_iri, geoproperty_iri.clone());
            let Some(candidates) = self.spatial.query(&key, &geo_q.georel, ref_geom) else {
                // Any cold partition forces a fall-through: we would otherwise
                // silently return an empty or incomplete set for that type.
                return Arc::clone(compiled);
            };
            any_warm = true;
            union.extend(candidates);
        }
        if !any_warm {
            return Arc::clone(compiled);
        }

        let candidate_ids: Vec<Uri> = union.into_iter().map(Uri::Urn).collect();

        let final_ids: Vec<Uri> = if compiled.selection.ids.is_empty() {
            candidate_ids
        } else {
            let user_set: HashSet<&str> = compiled.selection.ids.iter().map(AsRef::as_ref).collect();
            candidate_ids.into_iter().filter(|id| user_set.contains(AsRef::<str>::as_ref(id))).collect()
        };

        let mut cloned = (**compiled).clone();
        cloned.selection.ids = final_ids;
        cloned.selection.geo = None;
        tracing::debug!("spatial prefilter hit: {} candidate(s) for geoQ={:?}", cloned.selection.ids.len(), geo_q.georel);
        Arc::new(cloned)
    }

    async fn compile_geo_query(&self, params: &EntityQueryParameters, context: &ContextSpec) -> Result<Option<GeoQuery>, AndromedaError> {
        geo_compile::compile_geo_query(
            params.georel.as_deref(),
            params.geometry.as_deref(),
            params.coordinates.as_deref(),
            params.geoproperty.as_deref(),
            &ContextualExpander::new(&self.engine, context),
        )
        .await
        .map_err(AndromedaError::from)
    }

    async fn apply_projection(&self, entities: Vec<Entity>, query: &CompiledEntityQuery, context: &ContextSpec) -> Result<Vec<Entity>, AndromedaError> {
        if query.projection.pick.is_empty() && query.projection.omit.is_empty() && query.selection.attrs.is_empty() {
            return Ok(entities);
        }

        let mut projected = Vec::with_capacity(entities.len());
        for mut entity in entities {
            if !query.selection.attrs.is_empty() {
                self.filter_entity_attrs(&mut entity, &query.selection.attrs, context).await?;
            }

            if !query.projection.pick.is_empty() {
                let mut keys_to_remove = Vec::new();
                for key in entity.attrs.keys() {
                    let iri = self.expanded_key(key.as_str(), context).await?;
                    if !query.projection.pick.iter().any(|term| term.matches_attribute(key.as_str(), &iri)) {
                        keys_to_remove.push(key.clone()); // clone: owned key to avoid borrowing entity.attrs during removal
                    }
                }
                for k in keys_to_remove {
                    entity.attrs.shift_remove(&k);
                }
            }

            if !query.projection.omit.is_empty() {
                let mut keys_to_remove = Vec::new();
                for key in entity.attrs.keys() {
                    let iri = self.expanded_key(key.as_str(), context).await?;
                    if query.projection.omit.iter().any(|term| term.matches_attribute(key.as_str(), &iri)) {
                        keys_to_remove.push(key.clone()); // clone: owned key to avoid borrowing entity.attrs during removal
                    }
                }
                for k in keys_to_remove {
                    entity.attrs.shift_remove(&k);
                }
            }
            projected.push(entity);
        }

        Ok(projected)
    }

    async fn filter_entity_attrs(&self, entity: &mut Entity, wanted_iris: &[IriBuf], context: &ContextSpec) -> Result<(), AndromedaError> {
        let mut keys_to_remove = Vec::new();
        for key in entity.attrs.keys() {
            let iri = self.expanded_key(key.as_str(), context).await?;
            if !wanted_iris.contains(&iri) {
                keys_to_remove.push(key.clone()); // clone: owned key to avoid borrowing entity.attrs during removal
            }
        }
        for k in keys_to_remove {
            entity.attrs.shift_remove(&k);
        }
        Ok(())
    }

    /// Expand an attribute key of a compacted entity to the IRI it names.
    async fn expanded_key(&self, key: &str, context: &ContextSpec) -> Result<IriBuf, AndromedaError> {
        ContextualExpander::new(&self.engine, context)
            .expand_term(key)
            .await
            .map_err(AndromedaError::from)
    }
}

// ---------------------------------------------------------------------------
// Free functions (public API for handlers)
// ---------------------------------------------------------------------------

fn collect_relationship_objects(entity: &Entity, ids: &mut HashSet<String>) {
    for attr_val in entity.attrs.values() {
        match attr_val {
            AttributeValue::Single(attr) => match attr.as_ref() {
                Attribute::Relationship(r) => {
                    ids.insert(r.object.to_string());
                }
                Attribute::ListRelationship(lr) => {
                    for obj in &lr.object_list {
                        ids.insert(obj.to_string());
                    }
                }
                _other => {}
            },
            AttributeValue::Multiple(list) => {
                for attr in list {
                    match attr {
                        Attribute::Relationship(r) => {
                            ids.insert(r.object.to_string());
                        }
                        Attribute::ListRelationship(lr) => {
                            for obj in &lr.object_list {
                                ids.insert(obj.to_string());
                            }
                        }
                        _other => {}
                    }
                }
            }
            _other => {}
        }
    }
}

fn filter_entity_by_dataset_id(entity: &mut Entity, dataset_ids: &[String]) {
    let mut keys_to_remove = Vec::new();
    for (key, attr_val) in entity.attrs.iter_mut() {
        match attr_val {
            AttributeValue::Single(attr) => {
                let ds = attr.as_ref().dataset_id_string();
                let matches = dataset_ids.iter().any(|wanted| {
                    if wanted == "@none" {
                        ds.is_none()
                    } else {
                        ds.as_deref() == Some(wanted.as_str())
                    }
                });
                if !matches {
                    keys_to_remove.push(key.clone()); // clone: owned key to avoid borrowing entity.attrs during removal
                }
            }
            AttributeValue::Multiple(list) => {
                list.retain(|attr| {
                    let ds = attr.dataset_id_string();
                    dataset_ids.iter().any(|wanted| {
                        if wanted == "@none" {
                            ds.is_none()
                        } else {
                            ds.as_deref() == Some(wanted.as_str())
                        }
                    })
                });
                if list.is_empty() {
                    keys_to_remove.push(key.clone()); // clone: owned key to avoid borrowing entity.attrs during removal
                } else if list.len() == 1 {
                    *attr_val = AttributeValue::Single(Box::new(list.remove(0)));
                }
            }
            AttributeValue::Raw(_) => {}
        }
    }
    for k in keys_to_remove {
        entity.attrs.shift_remove(&k);
    }
}

/// Read an expanded `expandValues` / `jsonKeys` selection as the attribute
/// names a `q` filter is keyed by (clause 6.4.3.2).
fn opted_in_attribute_names(iris: &[IriBuf]) -> Result<HashSet<AttributeName>, QueryError> {
    iris.iter().map(expanded_attribute_name).collect()
}
