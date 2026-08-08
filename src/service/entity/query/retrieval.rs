//! Reading entities: clause 5.6.3 retrieval and clause 5.7.2 query.

use crate::{
    core::error::AndromedaError,
    service::entity::{
        compiled_cache::LimitCap,
        expanded_member::unwrap_expanded_ref,
        query::{
            EntityQueryService,
            dataset_filter::filter_entity_by_dataset_id,
            pushdown_refusal::refuse_unnarrowable,
            residual::finish_page,
            spatial_prefilter::SpatialNarrowing,
        },
    },
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::{entity::compiled::CompiledEntityQuery, pagination::CountPolicy};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
use serde_json::Value;
use std::collections::HashMap;
use urn_rs::Urn;

impl EntityQueryService {
    /// Retrieve a single entity by identifier (clause 5.6.3), optionally
    /// narrowing its attributes and attribute instances.
    pub async fn get_inner(
        &self,
        id: &Urn,
        attrs: &[String],
        sys_attrs: SysAttrsInclusion,
        dataset_ids: &[String],
        context: &ContextSpec,
    ) -> Result<Entity, AndromedaError> {
        let entity_uri = Uri::from(id.clone()); // clone: owned Urn -> Uri; the store is keyed by Uri
        let Some(expanded_entity) = self.repo.get(&entity_uri).await? else {
            return Err(AndromedaError::entity_not_found(entity_uri.as_str()));
        };

        let mut entity = self.compact_to_entity(expanded_entity, context, Some(entity_uri.as_str())).await?;

        if sys_attrs == SysAttrsInclusion::Include
            && let Some(snapshot) = self.repo.get_sys_attrs(&entity_uri).await?
        {
            entity = entity.with_sys_attrs(&snapshot);
        }

        if !attrs.is_empty() {
            let mut wanted_iris = Vec::with_capacity(attrs.len());
            for attr in attrs {
                wanted_iris.push(self.expanded_key(attr, context).await?);
            }
            self.filter_entity_attrs(&mut entity, &wanted_iris, context).await?;

            if entity.attrs.is_empty() {
                return Err(AndromedaError::entity_not_found(entity_uri.as_str()));
            }
        }

        if !dataset_ids.is_empty() {
            filter_entity_by_dataset_id(&mut entity, dataset_ids);
        }

        Ok(entity)
    }

    /// Execute a parameterized entity query (clause 5.7.2), returning matching
    /// entities and the optional clause 6.3.13 count.
    pub async fn query(&self, query: &EntityQueryParameters, context: &ContextSpec) -> Result<(Vec<Entity>, Option<i64>), AndromedaError> {
        let compiled = self.compile_query_cached(query, context, LimitCap::Capped).await?;
        // Gated on the narrowed selection, not the one the client wrote: the
        // spatial index *applies* a geo relation rather than declining it, so a
        // store that cannot narrow `georel` is never asked to when the index
        // already answered it.
        let narrowed = match self.spatial_prefilter(&compiled) {
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
        refuse_unnarrowable(self.repo.profile(), &narrowed.selection)?;
        let outcome = self.repo.query_compiled(&narrowed).await?;
        let page = finish_page(outcome, compiled.pagination)?;
        let sys_attrs = query.parse_options().sys_attrs;

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
            self.repo.get_sys_attrs_batch(&ids).await?
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

            let mut entity = self.compact_to_entity(value, context, None).await?;

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
            let dataset_ids = dataset_ids.iter().map(ToString::to_string).collect::<Vec<_>>();
            for entity in &mut entities {
                filter_entity_by_dataset_id(entity, &dataset_ids);
            }
        }

        Ok((entities, page.total_count))
    }

    /// Execute a compiled query returning only entity identifiers
    /// (clause 5.2.39 entity maps).
    pub async fn query_ids(&self, compiled: &CompiledEntityQuery) -> Result<Vec<String>, AndromedaError> {
        refuse_unnarrowable(self.repo.profile(), &compiled.selection)?;
        let outcome = self.repo.query_ids(compiled).await?;
        self.finish_ids(outcome, compiled.pagination).await
    }
}
