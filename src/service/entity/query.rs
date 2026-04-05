use crate::{
    domain::{
        attribute::{Attribute, AttributeValue},
        entity::Entity,
    },
    error::AndromedaError,
    jsonld::{CompactOptions, ContextSpec, JsonLdEngine},
    query::{
        compiled::{CompiledEntityQuery, CompiledOrderTerm, GeoQuery, IcuCollation, IdPattern, OrderingGeometry, ScopeQuery},
        entity::EntityQueryParameters,
        params::SharedQueryParams,
    },
    repository::entity::EntityRepository,
};
use serde_json::Value;
use sqlx::postgres::PgPool;
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
};
use urn::Urn;

/// Service for querying and retrieving NGSI-LD entities.
pub struct EntityQueryService {
    repo: EntityRepository,
    engine: JsonLdEngine,
}

type ResolveLinkedResult = Result<HashMap<String, Entity>, AndromedaError>;
type ResolveLinkedFuture<'a> = Pin<Box<dyn Future<Output = ResolveLinkedResult> + Send + 'a>>;

impl EntityQueryService {
    /// Create a new entity query service backed by the given connection pool.
    pub fn new(pool: &PgPool, engine: JsonLdEngine) -> EntityQueryService {
        let repo = EntityRepository::new(pool);
        EntityQueryService { repo, engine }
    }

    /// Return a reference to the underlying JSON-LD engine.
    pub fn engine(&self) -> &JsonLdEngine {
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
        .ok_or_else(|| AndromedaError::bad_request_data("Entity data is not a JSON object"))?;

        if let Some(t) = obj.remove("@type") {
            obj.insert("type".to_string(), t);
        }

        let id = id_override
            .map(|s| s.to_string())
            .or_else(|| obj.get("id").and_then(|v| v.as_str()).map(str::to_string))
            .or_else(|| obj.get("@id").and_then(|v| v.as_str()).map(str::to_string))
            .ok_or_else(|| AndromedaError::bad_request_data("Entity missing id field"))?;

        obj.remove("@id");
        obj.insert("id".to_string(), serde_json::json!(id));

        let val = Value::Object(obj);

        let mut entity: Entity = serde_json::from_value(val).map_err(|err| AndromedaError::bad_request_data(format!("Invalid entity data: {}", err)))?;

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
        sys_attrs: bool,
        dataset_ids: &[String],
        context: &ContextSpec,
    ) -> Result<Entity, AndromedaError> {
        let id_str = id.to_string();
        let result = self.repo.get(&id_str).await?;

        if let Some(expanded_entity) = result {
            let mut entity = self.compact_to_entity(expanded_entity, context, Some(&id_str)).await?;

            if sys_attrs && let Some(sa) = self.repo.get_sys_attrs(&id_str).await? {
                entity = entity.with_sys_attrs(&sa);
            }

            if !attrs.is_empty() {
                let mut wanted_iris = Vec::new();
                for a in attrs {
                    wanted_iris.push(self.engine.expand_term(a, context).await?);
                }
                self.filter_entity_attrs(&mut entity, &wanted_iris, context).await?;

                if entity.attrs.is_empty() {
                    return Err(AndromedaError::not_found(id_str));
                }
            }

            if !dataset_ids.is_empty() {
                filter_entity_by_dataset_id(&mut entity, dataset_ids);
            }

            return Ok(entity);
        }

        Err(AndromedaError::not_found(id_str))
    }

    /// Execute a parameterized entity query, returning matching entities and an optional total count.
    pub async fn query(&self, query: &EntityQueryParameters, context: &ContextSpec) -> Result<(Vec<Entity>, Option<i64>), AndromedaError> {
        let compiled = self.compile_query(query, context, true).await?;
        let query_result = self.repo.query_compiled(&compiled).await?;
        let sys_attrs = query.parse_options().sys_attrs;

        // Batch-fetch sys_attrs for all entities in one query instead of N+1
        let sys_attrs_map = if sys_attrs {
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

            if sys_attrs
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
    pub fn resolve_linked_entities<'a>(
        &'a self,
        entities: &'a [Entity],
        join_level: u32,
        sys_attrs: bool,
        context: &'a ContextSpec,
        contained_by: &'a HashSet<String>,
    ) -> ResolveLinkedFuture<'a> {
        const MAX_JOIN_LEVEL: u32 = 5;
        let join_level = join_level.min(MAX_JOIN_LEVEL);

        Box::pin(async move {
            if join_level == 0 {
                return Ok(HashMap::new());
            }

            let mut linked_ids: HashSet<String> = HashSet::new();
            let entity_ids: HashSet<String> = entities.iter().map(|e| e.id.to_string()).collect();

            for entity in entities {
                collect_relationship_objects(entity, &mut linked_ids);
            }
            linked_ids.retain(|id| !entity_ids.contains(id) && !contained_by.contains(id));

            // Batch-fetch all linked entities in one query instead of N+1
            let linked_id_vec: Vec<String> = linked_ids.iter().cloned().collect(); // clone: owned strings from HashSet for batch query API
            let batch_payloads = self.repo.get_batch(&linked_id_vec).await?;

            let batch_sys_attrs = if sys_attrs {
                self.repo.get_sys_attrs_batch(&linked_id_vec).await?
            } else {
                HashMap::new()
            };

            let mut linked_map = HashMap::new();
            for (linked_id, expanded) in batch_payloads {
                match self.compact_to_entity(expanded, context, Some(&linked_id)).await {
                    Ok(mut entity) => {
                        if sys_attrs && let Some(sa) = batch_sys_attrs.get(&linked_id) {
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
                let level2_entities: Vec<Entity> = linked_map.values().cloned().collect(); // clone: owned entities for recursive resolution
                let mut next_contained_by: HashSet<String> = contained_by.clone(); // clone: owned set for recursive call, caller retains original
                next_contained_by.extend(entity_ids);
                next_contained_by.extend(linked_map.keys().cloned()); // clone: owned keys for extending set
                let level2_linked = self
                    .resolve_linked_entities(&level2_entities, join_level - 1, sys_attrs, context, &next_contained_by)
                    .await?;
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
        self.compile_query(params, context, true).await
    }

    /// Compile an entity query without applying a limit cap.
    pub async fn compile_query_uncapped(&self, params: &EntityQueryParameters, context: &ContextSpec) -> Result<CompiledEntityQuery, AndromedaError> {
        self.compile_query(params, context, false).await
    }

    /// Execute a compiled query returning only entity IDs.
    pub async fn query_ids(&self, compiled: &CompiledEntityQuery) -> Result<Vec<String>, AndromedaError> {
        self.repo.query_ids(compiled).await
    }

    // -------------------------------------------------------------------------
    // Query compilation (private)
    // -------------------------------------------------------------------------

    async fn compile_query(&self, params: &EntityQueryParameters, context: &ContextSpec, cap_limit: bool) -> Result<CompiledEntityQuery, AndromedaError> {
        let ids = params.parse_ids();
        let id_pattern = params
            .id_pattern
            .as_deref()
            .map(IdPattern::new)
            .transpose()
            .map_err(|e| AndromedaError::bad_request_data(e.to_string()))?;

        let (types, type_match_all) = if let Some(raw_types) = params.entity_type.as_deref() {
            let raw_types = raw_types.trim();
            if raw_types.starts_with('(') && raw_types.ends_with(')') {
                // Semicolon-separated = match-all mode (spec 4.9)
                let inner = &raw_types[1..raw_types.len() - 1];
                let mut expanded = Vec::new();
                for t in inner.split(';').map(str::trim) {
                    if !t.is_empty() {
                        expanded.push(self.engine.expand_type(t, context).await?);
                    }
                }
                (Some(expanded), true)
            } else {
                let expanded = crate::query::compile::expand_entity_types(raw_types, &self.engine, context).await?;
                (Some(expanded), false)
            }
        } else {
            (None, false)
        };

        let attrs = crate::query::compile::expand_attrs(&params.parse_attrs(), &self.engine, context).await?;

        let q_expr = crate::query::compile::expand_q_expression(params.q.as_deref(), &self.engine, context).await?;

        // Apply expandValues: expand string RHS values for specified attributes (spec 6.4.3.2)
        let q_expr = if let Some(q_expr) = q_expr {
            if let Some(ref ev) = params.expand_values {
                let ev_attrs: Vec<String> = ev.split(',').map(|s| s.trim().to_string()).collect();
                let ev_iris = crate::query::compile::expand_attrs(&ev_attrs, &self.engine, context).await?;
                let ev_set: HashSet<String> = ev_iris.into_iter().collect();
                Some(crate::jsonld::term_resolver::apply_expand_values(&self.engine, q_expr, &ev_set, context).await?)
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
                let jk_iris = crate::query::compile::expand_attrs(&jk_attrs, &self.engine, context).await?;
                let jk_set: HashSet<String> = jk_iris.into_iter().collect();
                Some(crate::jsonld::term_resolver::apply_json_keys(&self.engine, q_expr, &jk_set, context).await?)
            } else {
                Some(q_expr)
            }
        } else {
            None
        };

        let geo = self.compile_geo_query(params, context).await?;
        let opts = params.parse_options();

        let pick = crate::query::compile::expand_projection_terms(&params.parse_pick(), &self.engine, context).await?;
        let omit = crate::query::compile::expand_projection_terms(&params.parse_omit(), &self.engine, context).await?;

        let order_by = {
            let raw = params.parse_order_by();
            let mut compiled = Vec::new();
            for term in &raw {
                let attr_iri = if term.attr == "id" || term.attr == "type" {
                    term.attr.clone() // clone: owned value for CompiledOrderTerm struct
                } else {
                    self.engine.expand_term(&term.attr, context).await?
                };
                compiled.push(CompiledOrderTerm {
                    attr_iri,
                    direction: term.direction,
                    collation: params.collation.as_ref().map(|c| IcuCollation::new(c.clone())), // clone: owned value for each CompiledOrderTerm in loop
                });
            }
            compiled
        };

        // Compile orderFrom/orderGeometry into OrderingGeometry (S5.2.43)
        let ordering_geometry = if let Some(ref order_from) = params.order_from {
            let coordinates: serde_json::Value = serde_json::from_str(order_from)
                .map_err(|_| AndromedaError::bad_request_data(format!("Invalid orderFrom value: '{}' is not valid JSON", order_from)))?;
            let geometry_type = params.order_geometry.as_deref().unwrap_or("Point");
            let geojson_value = serde_json::json!({
                "type": geometry_type,
                "coordinates": coordinates
            });
            let geometry: geojson::Geometry = serde_json::from_value(geojson_value)
                .map_err(|e| AndromedaError::bad_request_data(format!("Invalid orderFrom/orderGeometry combination: {}", e)))?;
            Some(OrderingGeometry { geometry })
        } else {
            None
        };

        // Validate: distance ordering requires a reference geometry (S5.7.2.4)
        let has_distance_term = order_by.iter().any(|t| t.direction.is_distance());
        if has_distance_term && geo.is_none() && ordering_geometry.is_none() {
            return Err(AndromedaError::bad_request_data(
                "Distance-based ordering requires either a geo-query or orderFrom parameter".to_string(),
            ));
        }

        let compiled = CompiledEntityQuery {
            ids,
            id_pattern,
            types,
            attrs,
            q_expr,
            geo,
            ordering_geometry,
            limit: if cap_limit {
                params.effective_limit()
            } else {
                params.effective_limit_uncapped()
            },
            offset: params.effective_offset(),
            count: opts.count,
            pick,
            omit,
            order_by,
            scope_q: params.scope_q.as_ref().map(|s| ScopeQuery::new(s.clone())), // clone: owned value for CompiledEntityQuery struct
            type_match_all,
        };

        tracing::info!("Compiled query: {:?}", compiled);

        Ok(compiled)
    }

    async fn compile_geo_query(&self, params: &EntityQueryParameters, context: &ContextSpec) -> Result<Option<GeoQuery>, AndromedaError> {
        crate::query::geo::compile_geo_query(
            params.georel.as_deref(),
            params.geometry.as_deref(),
            params.coordinates.as_deref(),
            params.geoproperty.as_deref(),
            &self.engine,
            context,
        )
        .await
    }

    async fn apply_projection(&self, entities: Vec<Entity>, query: &CompiledEntityQuery, context: &ContextSpec) -> Result<Vec<Entity>, AndromedaError> {
        if query.pick.is_empty() && query.omit.is_empty() && query.attrs.is_empty() {
            return Ok(entities);
        }

        let mut projected = Vec::with_capacity(entities.len());
        for mut entity in entities {
            if !query.attrs.is_empty() {
                self.filter_entity_attrs(&mut entity, &query.attrs, context).await?;
            }

            if !query.pick.is_empty() {
                let mut keys_to_remove = Vec::new();
                for key in entity.attrs.keys() {
                    let iri = self.engine.expand_term(key, context).await?;
                    if !query.pick.contains(&iri) && !query.pick.contains(key) {
                        keys_to_remove.push(key.clone()); // clone: owned key to avoid borrowing entity.attrs during removal
                    }
                }
                for k in keys_to_remove {
                    entity.attrs.shift_remove(&k);
                }
            }

            if !query.omit.is_empty() {
                let mut keys_to_remove = Vec::new();
                for key in entity.attrs.keys() {
                    let iri = self.engine.expand_term(key, context).await?;
                    if query.omit.contains(&iri) || query.omit.contains(key) {
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

    async fn filter_entity_attrs(&self, entity: &mut Entity, wanted_iris: &[String], context: &ContextSpec) -> Result<(), AndromedaError> {
        let mut keys_to_remove = Vec::new();
        for key in entity.attrs.keys() {
            let iri = self.engine.expand_term(key, context).await?;
            if !wanted_iris.contains(&iri) {
                keys_to_remove.push(key.clone()); // clone: owned key to avoid borrowing entity.attrs during removal
            }
        }
        for k in keys_to_remove {
            entity.attrs.shift_remove(&k);
        }
        Ok(())
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
