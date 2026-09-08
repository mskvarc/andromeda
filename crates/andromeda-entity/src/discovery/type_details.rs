//! Retrieve Details of Available Entity Types (clause 5.7.6).

use crate::{
    discovery::{
        DiscoveryScope,
        EntityDiscoveryService,
        compaction_fixups::strip_ngsild_prefix,
        registry_union::union_type_summaries,
        stored_names::parse_discovery_iri,
    },
    error::EntityError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_spec::ContextSpec, engine::options::CompactOptions};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_model::entity_type_list::{EntityType, EntityTypeType};
use serde_json::Value;

impl EntityDiscoveryService {
    /// Return detailed information for all entity types, including their
    /// attribute names (clause 5.2.25). A type only the registry knows is
    /// reported with the attribute names registered beside it, which may be
    /// none (clause 5.7.11).
    pub async fn type_details(&self, target: &ContextTarget, scope: DiscoveryScope, context: &ContextSpec) -> Result<Value, EntityError> {
        // Keyset-paginate the aggregation so a very large set of distinct
        // types does not require a single large result set.
        const PAGE: u32 = 500;
        let mut summaries: Vec<(EntityTypeName, Vec<AttributeName>)> = Vec::new();
        let mut cursor: Option<EntityTypeName> = None;
        loop {
            let page = self.store.get_entity_type_summaries(target, cursor.as_ref(), PAGE).await?;
            if page.is_empty() {
                break;
            }
            cursor = page.last().map(|(t, _)| t.clone()); // clone: own cursor for next loop iteration
            let page_len = page.len();
            summaries.extend(page);
            if page_len < usize::try_from(PAGE).unwrap_or(usize::MAX) {
                break;
            }
        }
        union_type_summaries(&mut summaries, self.registered_types(scope).await?);

        let entity_types: Vec<EntityType> = summaries
            .into_iter()
            .map(|(type_iri, attrs)| {
                Ok(EntityType {
                    id: parse_discovery_iri(type_iri.to_string())?,
                    type_name: type_iri,
                    type_: EntityTypeType,
                    attribute_names: attrs,
                })
            })
            .collect::<Result<_, EntityError>>()?;

        // Expand and compact
        let expanded = serde_json::json!(entity_types.iter().map(jsonld::Expandable::expand::<serde_json::Value>).collect::<Vec<_>>());
        let mut compacted = self.engine.compact(expanded, context, CompactOptions::default()).await?;

        // Extract the @graph array (compaction wraps multiple items in @graph)
        if let Some(graph) = compacted.as_object_mut().and_then(|obj| obj.remove("@graph")) {
            compacted = graph;
        }

        strip_ngsild_prefix(&mut compacted);

        Ok(compacted)
    }
}
