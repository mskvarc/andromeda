//! Purging attributes from a set of entities (clause 5.6.21).
//!
//! Each entity is its own atomic write: the payload rewrite and the tombstones
//! for the purged instances commit together, and one entity failing must not
//! undo the ones already purged. The diff is computed from a payload read
//! outside that write, so the write states the `modifiedAt` it assumed and is
//! recomputed against a fresh read if a concurrent writer got in first.

use crate::{
    core::error::AndromedaError,
    service::entity::{
        EntityService,
        atomic_write::AtomicWrite,
        entity_mutation::{EntityMutation, EntityRecord},
        expanded_member::attr_name_of,
        history_record::HistoryRecord,
    },
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    expanded::{entity_member::extract_types, instance_key::removed_attribute_instances},
    notification::NotificationTrigger,
};
use andromeda_ngsi_ld_query::projection::purge::PurgeMode;
use serde_json::Value;
use std::sync::Arc;

impl EntityService {
    /// Purge attributes from entities according to the given mode (clause 5.6.21).
    ///
    /// - `Drop`: remove the listed attributes from each entity
    /// - `Keep`: retain only the listed attributes, removing all others
    ///
    /// Each entity is its own atomic write: the payload rewrite and the
    /// tombstones for the purged instances commit together, and one entity
    /// failing must not undo the ones already purged. The diff is computed
    /// from a payload read outside that write, so the write states the
    /// `modifiedAt` it assumed and is re-read if a concurrent writer got in.
    ///
    /// Returns the number of entities modified.
    pub async fn purge_attrs(&self, entity_ids: &[String], mode: &PurgeMode, context: &ContextSpec) -> Result<u32, AndromedaError> {
        let attr_iris: Vec<String> = match mode {
            PurgeMode::DeleteEntities => return Ok(0),
            PurgeMode::Drop(attrs) | PurgeMode::Keep(attrs) => {
                let mut expanded = Vec::with_capacity(attrs.len());
                for attr in attrs {
                    expanded.push(self.engine.expand_term(attr.as_str(), context).await?);
                }
                expanded
            }
        };

        let mut modified_count = 0u32;

        for entity_id in entity_ids {
            let mut purged: Option<PurgedEntity> = None;
            let outcome = self
                .commit_read_modify_write(entity_id, |previous| {
                    let Some((write, removed)) = build_purge_write(entity_id, previous, mode, &attr_iris)? else {
                        return Ok(None);
                    };
                    purged = Some(removed);
                    Ok(Some(write))
                })
                .await?;

            let (Some(_), Some(purged)) = (outcome, purged) else {
                continue;
            };
            modified_count += 1;

            // A purged attribute is a deleted attribute, and clause 5.2.14's
            // `attributeDeleted` trigger says what kind of change fires a
            // notification rather than which operation caused it. Reported the
            // way clause 5.6.5 reports the single-attribute form.
            let PurgedEntity {
                merged,
                previous,
                removed_names,
            } = purged;
            match entity_id.parse::<Uri>() {
                Ok(entity_uri) => self.emit_attr_change(
                    entity_uri,
                    extract_types(&merged),
                    NotificationTrigger::AttributeDeleted,
                    removed_names,
                    previous,
                    Some(merged),
                ),
                Err(_error) => {
                    tracing::error!(entity_id = %entity_id, event = "entity_uri_parse_failed");
                }
            }
        }

        Ok(modified_count)
    }
}

/// What one clause 5.6.21 purge removed from one entity.
struct PurgedEntity {
    /// The entity as it stands once the purged attributes are gone.
    merged: Arc<Value>,
    /// The payload as it stood before, when a `showChanges` subscriber needs it.
    previous: Option<Value>,
    /// The attributes the purge removed, which the `attributeDeleted`
    /// notification names.
    removed_names: Vec<AttributeName>,
}

/// The write that purges `previous` of the attributes `mode` selects, together
/// with what it removed, or `None` when the entity holds none of them.
fn build_purge_write(entity_id: &str, previous: &Value, mode: &PurgeMode, attr_iris: &[String]) -> Result<Option<(AtomicWrite, PurgedEntity)>, AndromedaError> {
    let mut purged = previous.clone(); // clone: the "before" side stays intact for the removal diff
    let obj = match &mut purged {
        Value::Array(arr) => arr.first_mut().and_then(|v| v.as_object_mut()),
        Value::Object(obj) => Some(obj),
        _other => None,
    };

    let Some(obj) = obj else {
        return Ok(None);
    };

    let keys_to_remove: Vec<String> = match mode {
        PurgeMode::Drop(_) => {
            attr_iris.iter().filter(|iri| obj.contains_key(iri.as_str())).cloned().collect() // clone: owned keys for removal
        }
        PurgeMode::Keep(_) => obj
            .keys()
            .filter(|k| {
                // Preserve JSON-LD structural keys and NGSI-LD system attrs
                !k.starts_with('@') && !attr_iris.iter().any(|iri| k.as_str() == iri)
            })
            .cloned() // clone: owned keys for removal
            .collect(),
        PurgeMode::DeleteEntities => Vec::new(),
    };

    if keys_to_remove.is_empty() {
        return Ok(None);
    }

    for key in &keys_to_remove {
        obj.remove(key);
    }

    let mut removed_names = Vec::with_capacity(keys_to_remove.len());
    for key in &keys_to_remove {
        removed_names.push(attr_name_of(key)?);
    }

    let removed = removed_attribute_instances(previous, &purged);
    let mut history = Vec::with_capacity(removed.len());
    for key in removed {
        history.push(HistoryRecord::Tombstone(key));
    }

    let merged = Arc::new(purged);
    let write = AtomicWrite::builder()
        .entity_id(Arc::from(entity_id))
        .entity(EntityMutation::Update(EntityRecord::from_expanded(Arc::clone(&merged)))) // clone: Arc ref-count bump; the payload is also the notification's
        .history(history)
        .build();

    Ok(Some((
        write,
        PurgedEntity {
            merged,
            previous: Some(previous.clone()), // clone: the "before" side a showChanges subscriber diffs against
            removed_names,
        },
    )))
}
