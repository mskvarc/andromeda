use crate::{
    domain::{
        attribute::{Attribute, AttributeValue},
        entity::Entity,
    },
    error::AndromedaError,
    jsonld::{CompactOptions, ContextSpec, JsonLdEngine},
    query::{
        compiled::{CompiledEntityQuery, CompiledOrderTerm, GeoQuery},
        entity::EntityQueryParameters,
    },
    repository::entity::EntityRepository,
};
use ngsi_ld_query::params::SharedQueryParams;
use serde_json::Value;
use sqlx::postgres::PgPool;
use std::collections::{HashMap, HashSet};
use urn::Urn;

pub struct EntityQueryService {
    repo: EntityRepository,
    engine: JsonLdEngine,
}

impl EntityQueryService {
    pub fn new(pool: &PgPool, engine: JsonLdEngine) -> Self {
        let repo = EntityRepository::new(pool);
        Self { repo, engine }
    }

    pub fn engine(&self) -> &JsonLdEngine {
        &self.engine
    }

    async fn compact_to_entity(&self, expanded: Value, context: &ContextSpec, id_override: Option<&str>) -> Result<Entity, AndromedaError> {
        let compacted = self.engine.compact(expanded, context, CompactOptions::default()).await?;

        let mut obj = match compacted {
            Value::Array(ref arr) => arr.first().and_then(|v| v.as_object()).cloned(),
            Value::Object(ref map) => Some(map.clone()),
            _ => None,
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

        serde_json::from_value(val).map_err(|err| AndromedaError::bad_request_data(format!("Invalid entity data: {}", err)))
    }

    // -------------------------------------------------------------------------
    // Read operations
    // -------------------------------------------------------------------------

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

        if let Some(mut expanded_entity) = result {
            if sys_attrs {
                if let Some((created_at, modified_at, expires_at)) = self.repo.get_sys_attrs(&id_str).await? {
                    super::inject_sys_attrs(&mut expanded_entity, created_at, modified_at, expires_at);
                }
            }

            let mut entity = self.compact_to_entity(expanded_entity, context, Some(&id_str)).await?;

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
                        _ => None,
                    };
                    obj.and_then(|o| o.get("@id").and_then(|v| v.as_str()).map(String::from))
                })
                .collect();
            self.repo.get_sys_attrs_batch(&ids).await?
        } else {
            HashMap::new()
        };

        let mut entities = Vec::new();
        for mut value in query_result.rows {
            if sys_attrs {
                let id_str = super::expanded_entity_object_mut(&mut value).and_then(|obj| obj.get("@id").and_then(|v| v.as_str()).map(String::from));
                if let Some(ref id_str) = id_str {
                    if let Some(&(created_at, modified_at, expires_at)) = sys_attrs_map.get(id_str) {
                        super::inject_sys_attrs(&mut value, created_at, modified_at, expires_at);
                    }
                }
            }

            let entity = self.compact_to_entity(value, context, None).await?;
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

    pub fn resolve_linked_entities<'a>(
        &'a self,
        entities: &'a [Entity],
        join_level: u32,
        sys_attrs: bool,
        context: &'a ContextSpec,
        contained_by: &'a HashSet<String>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<HashMap<String, Entity>, AndromedaError>> + Send + 'a>> {
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
            let linked_id_vec: Vec<String> = linked_ids.iter().cloned().collect();
            let batch_payloads = self.repo.get_batch(&linked_id_vec).await?;

            let batch_sys_attrs = if sys_attrs {
                self.repo.get_sys_attrs_batch(&linked_id_vec).await?
            } else {
                HashMap::new()
            };

            let mut linked_map = HashMap::new();
            for (linked_id, mut expanded) in batch_payloads {
                if sys_attrs {
                    if let Some(&(created_at, modified_at, expires_at)) = batch_sys_attrs.get(&linked_id) {
                        super::inject_sys_attrs(&mut expanded, created_at, modified_at, expires_at);
                    }
                }
                match self.compact_to_entity(expanded, context, Some(&linked_id)).await {
                    Ok(entity) => {
                        linked_map.insert(linked_id, entity);
                    }
                    Err(e) => {
                        tracing::debug!("Failed to compact linked entity {}: {}", linked_id, e);
                    }
                }
            }

            if join_level > 1 && !linked_map.is_empty() {
                let level2_entities: Vec<Entity> = linked_map.values().cloned().collect();
                let mut next_contained_by: HashSet<String> = contained_by.clone();
                next_contained_by.extend(entity_ids);
                next_contained_by.extend(linked_map.keys().cloned());
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

    pub async fn compile_query_public(&self, params: &EntityQueryParameters, context: &ContextSpec) -> Result<CompiledEntityQuery, AndromedaError> {
        self.compile_query(params, context, true).await
    }

    pub async fn compile_query_uncapped(&self, params: &EntityQueryParameters, context: &ContextSpec) -> Result<CompiledEntityQuery, AndromedaError> {
        self.compile_query(params, context, false).await
    }

    pub async fn query_ids(&self, compiled: &CompiledEntityQuery) -> Result<Vec<String>, AndromedaError> {
        self.repo.query_ids(compiled).await
    }

    // -------------------------------------------------------------------------
    // Query compilation (private)
    // -------------------------------------------------------------------------

    async fn compile_query(&self, params: &EntityQueryParameters, context: &ContextSpec, cap_limit: bool) -> Result<CompiledEntityQuery, AndromedaError> {
        let ids = params.parse_ids();
        let id_pattern = params.id_pattern.clone();

        if let Some(ref pattern) = id_pattern {
            crate::query::compile::validate_id_pattern(pattern)?;
        }

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
                let attr_iri = if term.attr == "id" || term.attr == "type" || term.direction.is_distance() {
                    term.attr.clone()
                } else {
                    self.engine.expand_term(&term.attr, context).await?
                };
                compiled.push(CompiledOrderTerm {
                    attr_iri,
                    direction: term.direction,
                    collation: params.collation.clone(),
                });
            }
            compiled
        };

        let compiled = CompiledEntityQuery {
            ids,
            id_pattern,
            types,
            attrs,
            q_expr,
            geo,
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
            scope_q: params.scope_q.clone(),
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
                        keys_to_remove.push(key.clone());
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
                        keys_to_remove.push(key.clone());
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
                keys_to_remove.push(key.clone());
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
            AttributeValue::Single(Attribute::Relationship(r)) => {
                ids.insert(r.object.to_string());
            }
            AttributeValue::Single(Attribute::ListRelationship(lr)) => {
                for obj in &lr.object_list {
                    ids.insert(obj.to_string());
                }
            }
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
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

fn filter_entity_by_dataset_id(entity: &mut Entity, dataset_ids: &[String]) {
    let mut keys_to_remove = Vec::new();
    for (key, attr_val) in entity.attrs.iter_mut() {
        match attr_val {
            AttributeValue::Single(attr) => {
                let ds = attr.dataset_id_string();
                let matches = dataset_ids.iter().any(|wanted| {
                    if wanted == "@none" {
                        ds.is_none()
                    } else {
                        ds.as_deref() == Some(wanted.as_str())
                    }
                });
                if !matches {
                    keys_to_remove.push(key.clone());
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
                    keys_to_remove.push(key.clone());
                } else if list.len() == 1 {
                    *attr_val = AttributeValue::Single(list.remove(0));
                }
            }
            AttributeValue::Raw(_) => {}
        }
    }
    for k in keys_to_remove {
        entity.attrs.shift_remove(&k);
    }
}
