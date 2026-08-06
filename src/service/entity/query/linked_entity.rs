//! Resolving the entities a result set links to (clause 4.5.20 `join`).

use crate::{core::error::AndromedaError, service::entity::query::EntityQueryService};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_model::{
    attribute::instance::{Attribute, AttributeValue},
    entity::Entity,
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
};

/// The entities one join level resolved, keyed by identifier.
type ResolveLinkedResult = Result<HashMap<String, Entity>, AndromedaError>;
/// The boxed future the recursive walk returns.
type ResolveLinkedFuture<'a> = Pin<Box<dyn Future<Output = ResolveLinkedResult> + Send + 'a>>;

/// The deepest join clause 4.5.20 is followed to.
///
/// Bounded because a relationship graph may be cyclic and the parameter is
/// client-supplied.
const MAX_JOIN_LEVEL: u32 = 5;

impl EntityQueryService {
    /// Recursively resolve linked entities up to the given join level.
    ///
    /// The recursive work runs on an owned `contained_by` set that is mutated
    /// in place as each level adds the identifiers it has just resolved, so no
    /// per-level set copy occurs and the walk holds `&Entity` rather than
    /// cloning entities.
    pub fn resolve_linked_entities<'a>(
        &'a self,
        entities: &'a [Entity],
        join_level: u32,
        sys_attrs: SysAttrsInclusion,
        context: &'a ContextSpec,
        contained_by: &HashSet<String>,
    ) -> ResolveLinkedFuture<'a> {
        let join_level = join_level.min(MAX_JOIN_LEVEL);
        let refs: Vec<&'a Entity> = entities.iter().collect();
        let owned = contained_by.clone(); // clone: the recursion owns and mutates the set across levels
        Box::pin(self.resolve_linked_inner(refs, join_level, sys_attrs, context, owned))
    }

    fn resolve_linked_inner<'a>(
        &'a self,
        entities: Vec<&'a Entity>,
        join_level: u32,
        sys_attrs: SysAttrsInclusion,
        context: &'a ContextSpec,
        mut contained_by: HashSet<String>,
    ) -> ResolveLinkedFuture<'a> {
        Box::pin(async move {
            if join_level == 0 {
                return Ok(HashMap::new());
            }

            let mut linked_ids: HashSet<String> = HashSet::new();
            let entity_ids: HashSet<String> = entities.iter().map(|entity| entity.id.to_string()).collect();

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
                            && let Some(snapshot) = batch_sys_attrs.get(&linked_id)
                        {
                            entity = entity.with_sys_attrs(snapshot);
                        }
                        linked_map.insert(linked_id, entity);
                    }
                    Err(error) => {
                        tracing::debug!("Failed to compact linked entity {}: {}", linked_id, error);
                    }
                }
            }

            if join_level > 1 && !linked_map.is_empty() {
                contained_by.extend(entity_ids);
                contained_by.reserve(linked_map.len());
                for key in linked_map.keys() {
                    contained_by.insert(key.clone()); // clone: the cycle guard outlives this borrow of the map
                }
                let next_level: Vec<&Entity> = linked_map.values().collect();
                let resolved = self.resolve_linked_inner(next_level, join_level - 1, sys_attrs, context, contained_by).await?;
                linked_map.extend(resolved);
            }

            Ok(linked_map)
        })
    }
}

/// Collect every identifier the entity's relationships point at.
fn collect_relationship_objects(entity: &Entity, ids: &mut HashSet<String>) {
    for attr_val in entity.attrs.values() {
        match attr_val {
            AttributeValue::Single(attr) => collect_from_attribute(attr.as_ref(), ids),
            AttributeValue::Multiple(list) => {
                for attr in list {
                    collect_from_attribute(attr, ids);
                }
            }
            AttributeValue::Raw(_) => {}
        }
    }
}

/// Collect the identifiers one attribute instance points at.
fn collect_from_attribute(attr: &Attribute, ids: &mut HashSet<String>) {
    match attr {
        Attribute::Relationship(relationship) => {
            ids.insert(relationship.object.to_string());
        }
        Attribute::ListRelationship(list) => {
            for object in &list.object_list {
                ids.insert(object.to_string());
            }
        }
        Attribute::Property(_)
        | Attribute::GeoProperty(_)
        | Attribute::JsonProperty(_)
        | Attribute::LanguageProperty(_)
        | Attribute::ListProperty(_)
        | Attribute::VocabProperty(_) => {}
    }
}
