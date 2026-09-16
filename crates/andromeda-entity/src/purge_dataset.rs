//! Dataset-scoped attribute purge (the Andromeda `purge_dataset_scope`
//! extension; see `docs/extensions/purge-dataset-scope.md`).
//!
//! The standard Purge (clause 5.6.21) `drop`/`keep` remove whole attributes.
//! This variant removes only the `datasetId` instances a selector names, reusing
//! the exact clause-5.6.5 mutation the single-entity Delete Attribute uses
//! ([`delete_attr`]). Whole-attribute purging stays in [`crate::purge`]; this
//! module is reached only for a `datasetId` selector.

use crate::purge::PurgedEntity;
use andromeda_context_commit::{atomic_write::AtomicWrite, entity_mutation::EntityMutation, entity_record::EntityRecord, history_record::HistoryRecord};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::expanded::{
    mutation::{attribute::delete_attr, error::ExpandedEntityError, selection::DatasetSelection},
    node::expanded_object_mut,
    removed_instance::removed_attribute_instances,
};
use andromeda_ngsi_ld_query::{dataset::DatasetSelector, projection::purge::PurgeMode};
use iri_rs::IriBuf;
use serde_json::Value;
use std::sync::Arc;

/// The write that removes the selector's `datasetId` instances of the targeted
/// attributes from one entity, or `None` when nothing matched.
///
/// For `drop`, the targeted attributes are the listed ones; for `keep`, they are
/// every attribute the entity carries except the listed ones. Only the named
/// instances of each are removed, and the tombstones are computed from the
/// before/after diff exactly as the whole-attribute purge does.
pub(crate) fn build_dataset_purge_write(
    entity_id: &Uri,
    previous: &Value,
    mode: &PurgeMode,
    attr_iris: &[IriBuf],
    selector: &DatasetSelector,
) -> Option<(AtomicWrite, PurgedEntity)> {
    let mut purged = previous.clone(); // clone: the "before" side stays intact for the removal diff
    let targets = target_attributes(&mut purged, mode, attr_iris)?;
    if targets.is_empty() {
        return None;
    }

    let selection = selection_of(selector);
    for iri in &targets {
        match delete_attr(&mut purged, iri, selection) {
            // Success, or a benign miss: the entity carries the attribute but
            // not the named instance, or not the attribute at all, which is
            // routine across a set of entities rather than an error.
            Ok(())
            | Err(
                ExpandedEntityError::AttributeNotFound { .. }
                | ExpandedEntityError::AttributeHasNoInstances { .. }
                | ExpandedEntityError::DatasetNotFound { .. }
                | ExpandedEntityError::DefaultInstanceNotFound { .. },
            ) => {}
            // A payload that is not a well-formed expanded entity node cannot be
            // purged; leaving it untouched is safer than committing a malformed
            // write.
            Err(
                ExpandedEntityError::EntityNotExpanded
                | ExpandedEntityError::PatchNotExpanded
                | ExpandedEntityError::AttributesNotObject
                | ExpandedEntityError::InvalidAttributeName { .. }
                | ExpandedEntityError::InvalidAttributeType
                | ExpandedEntityError::AttributeTypeMismatch { .. },
            ) => return None,
        }
    }

    let removed = removed_attribute_instances(previous, &purged);
    if removed.is_empty() {
        return None;
    }
    let history: Vec<HistoryRecord> = removed.into_iter().map(HistoryRecord::Tombstone).collect();

    let merged = Arc::new(purged);
    let write = AtomicWrite::builder()
        .entity_id(Arc::new(entity_id.clone())) // clone: the write owns a shared validated identifier.
        .entity(EntityMutation::Update(EntityRecord::from_expanded(Arc::clone(&merged)))) // clone: Arc ref-count bump; the payload is also the notification's
        .history(history)
        .build();
    Some((write, PurgedEntity { merged, events: Vec::new() }))
}

/// The attribute IRIs one entity's dataset purge targets.
///
/// `drop` targets the listed attributes present on the entity; `keep` targets
/// every attribute except the listed ones, leaving the JSON-LD structural keys
/// and system members alone.
fn target_attributes(purged: &mut Value, mode: &PurgeMode, attr_iris: &[IriBuf]) -> Option<Vec<IriBuf>> {
    let object = expanded_object_mut(purged)?;
    let targets = match mode {
        PurgeMode::DeleteEntities => Vec::new(),
        PurgeMode::Drop { .. } => attr_iris.iter().filter(|iri| object.contains_key(iri.as_str())).cloned().collect(),
        PurgeMode::Keep { .. } => object
            .keys()
            .filter(|key| !key.starts_with('@') && !attr_iris.iter().any(|iri| key.as_str() == iri.as_str()))
            .filter_map(|key| IriBuf::new(key.clone()).ok()) // a payload key is an expanded IRI; a malformed one names no attribute to purge
            .collect(),
    };
    Some(targets)
}

/// The clause-5.6.5 selection the `datasetId` selector lowers to.
///
/// `@none` selects the default instance; a URI selects that one dataset.
/// `deleteAll` is not reached here — it removes the whole attribute and is
/// handled by the whole-attribute purge path.
const fn selection_of(selector: &DatasetSelector) -> DatasetSelection<'_> {
    match selector {
        DatasetSelector::Default => DatasetSelection::Default,
        DatasetSelector::Id(uri) => DatasetSelection::Single(uri),
    }
}
