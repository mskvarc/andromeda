//! Purging attributes from a set of entities (clause 5.6.21).
//!
//! Each entity is its own atomic write: the payload rewrite and the tombstones
//! for the purged instances commit together, and one entity failing must not
//! undo the ones already purged. The diff is computed from a payload read
//! outside that write, so the write states the `modifiedAt` it assumed and is
//! recomputed against a fresh read if a concurrent writer got in first.

use crate::{
    EntityService,
    error::EntityError,
    expanded_member::stored_entity_types,
    notification_plan::modification,
    purge_dataset::build_dataset_purge_write,
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    change_event::ChangeEvent,
    entity_mutation::EntityMutation,
    entity_record::EntityRecord,
    history_record::HistoryRecord,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::expanded::removed_instance::removed_attribute_instances;
use andromeda_ngsi_ld_query::projection::purge::{DatasetPurgeScope, PurgeMode};
use iri_rs::IriBuf;
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
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::JsonLd`] when one of the named attributes cannot
    /// be expanded against `context`, [`EntityError::Contention`] when
    /// concurrent writers superseded an entity's purge on every attempt, and
    /// [`EntityError::Persistence`] when the store could not read an entity or
    /// commit its purge.
    pub async fn purge_attrs(&self, target: &ContextTarget, entity_ids: &[Uri], mode: &PurgeMode, context: &ContextSpec) -> Result<u32, EntityError> {
        let prepared_context = self.engine.prepare_term_context(context);
        let attr_iris: Vec<IriBuf> = match mode {
            PurgeMode::DeleteEntities => return Ok(0),
            PurgeMode::Drop { attributes, .. } | PurgeMode::Keep { attributes, .. } => {
                let mut expanded = Vec::with_capacity(attributes.len());
                for attr in attributes {
                    expanded.push(self.engine.expand_term(attr.as_str(), &prepared_context).await?);
                }
                expanded
            }
        };

        let mut modified_count = 0u32;

        for entity_id in entity_ids {
            let mut purged: Option<PurgedEntity> = None;
            let outcome = self
                .commit_read_modify_write(target, entity_id, |previous| {
                    let Some((mut write, mut removed)) = build_scoped_purge_write(entity_id, previous, mode, &attr_iris)? else {
                        return Ok(None);
                    };
                    let change = modification(previous, &removed.merged)?;
                    let plan = self.plan_notifications(
                        target,
                        entity_id,
                        &stored_entity_types(&removed.merged)?,
                        change,
                        Some(&removed.merged),
                        Some(previous),
                    );
                    write.subscription_matches = plan.matches;
                    removed.events = plan.events;
                    purged = Some(removed);
                    Ok(Some(write))
                })
                .await?;

            let (Some(_), Some(purged)) = (outcome, purged) else {
                continue;
            };
            modified_count += 1;

            // A purged attribute is a deleted attribute, and clause 5.2.12's
            // `attributeDeleted` trigger says what kind of change fires a
            // notification rather than which operation caused it. Reported the
            // way clause 5.6.5 reports the single-attribute form.
            purged.events.into_iter().for_each(|event| self.emit_change(target, event));
        }

        Ok(modified_count)
    }
}

/// What one clause 5.6.21 purge removed from one entity.
pub(crate) struct PurgedEntity {
    /// The entity as it stands once the purged attributes are gone.
    pub(crate) merged: Arc<Value>,
    /// In-process events matching the durable records committed with the write.
    pub(crate) events: Vec<ChangeEvent>,
}

/// The write that purges one entity, dispatched on the dataset scope.
///
/// A whole-attribute scope — the standard clause 5.6.21 default and the
/// explicit `deleteAll`, which both remove every instance — takes the payload
/// key-removal path. A `datasetId` selector removes only the named instances of
/// each targeted attribute, reusing the single-entity clause 5.6.5 mutation.
///
/// # Errors
///
/// Reports [`EntityError::Internal`] when the stored payload's members cannot
/// be read back into the write's record or tombstones.
fn build_scoped_purge_write(
    entity_id: &Uri,
    previous: &Value,
    mode: &PurgeMode,
    attr_iris: &[IriBuf],
) -> Result<Option<(AtomicWrite, PurgedEntity)>, EntityError> {
    match mode.dataset_scope() {
        // Whole-entity deletion never reaches attribute purging.
        None => Ok(None),
        // `deleteAll` removes every instance, which is the whole attribute, so
        // it reduces to the standard key-removal path rather than a per-instance
        // one; only a `datasetId` selector needs the instance-level mutation.
        Some(DatasetPurgeScope::Unfiltered | DatasetPurgeScope::All) => build_purge_write(entity_id, previous, mode, attr_iris),
        Some(DatasetPurgeScope::Selector(selector)) => build_dataset_purge_write(entity_id, previous, mode, attr_iris, selector),
    }
}

/// The write that purges `previous` of the whole attributes `mode` selects,
/// together with what it removed, or `None` when the entity holds none of them.
///
/// # Errors
///
/// Reports [`EntityError::Internal`] when the stored payload's members cannot
/// be read back into the write's record or tombstones.
fn build_purge_write(entity_id: &Uri, previous: &Value, mode: &PurgeMode, attr_iris: &[IriBuf]) -> Result<Option<(AtomicWrite, PurgedEntity)>, EntityError> {
    let mut purged = previous.clone(); // clone: the "before" side stays intact for the removal diff
    let obj = match &mut purged {
        Value::Array(arr) => arr.first_mut().and_then(|v| v.as_object_mut()),
        Value::Object(obj) => Some(obj),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    };

    let Some(obj) = obj else {
        return Ok(None);
    };

    let keys_to_remove: Vec<String> = match mode {
        PurgeMode::Drop { .. } => attr_iris
            .iter()
            .filter(|iri| obj.contains_key(iri.as_str()))
            .map(|iri| iri.as_str().to_owned())
            .collect(),
        PurgeMode::Keep { .. } => obj
            .keys()
            .filter(|k| {
                // Preserve JSON-LD structural keys and NGSI-LD system attrs
                !k.starts_with('@') && !attr_iris.iter().any(|iri| k.as_str() == iri.as_str())
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

    let removed = removed_attribute_instances(previous, &purged).map_err(EntityError::internal)?;
    let mut history = Vec::with_capacity(removed.len());
    for instance in removed {
        history.push(HistoryRecord::Tombstone(instance));
    }

    let merged = Arc::new(purged);
    let write = AtomicWrite::builder()
        .entity_id(Arc::new(entity_id.clone())) // clone: the write owns a shared validated identifier.
        .entity(EntityMutation::Update(EntityRecord::from_expanded(Arc::clone(&merged))?)) // clone: Arc ref-count bump; the payload is also the notification's
        .history(history)
        .build();

    Ok(Some((write, PurgedEntity { merged, events: Vec::new() })))
}
