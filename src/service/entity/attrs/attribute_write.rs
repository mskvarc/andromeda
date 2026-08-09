//! The shape of an attribute-level write and what it produces.
//!
//! All five operations write the entity's whole payload back and differ only in
//! how they computed it, so they share one write constructor. What they do *not*
//! share is what the merge produced for the notification, which is why there are
//! two result types rather than one bag of optional fields.

use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    entity_mutation::{EntityMutation, EntityRecord},
    history_record::HistoryRecord,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::notification::NotificationTrigger;
use serde_json::Value;
use std::sync::Arc;

/// The write that stores `merged` as the entity's whole payload.
///
/// No precondition is set here:
/// [`commit_computed_write`](crate::service::entity::EntityService::commit_computed_write)
/// stamps it with the `modifiedAt` of the payload the mutation was computed
/// from, which is what stops a concurrent writer's change being overwritten.
pub(super) fn payload_write(id: &Uri, merged: &Arc<Value>, history: Vec<HistoryRecord>) -> AtomicWrite {
    AtomicWrite::builder()
        .entity_id(Arc::new(id.clone())) // clone: the write owns a shared validated identifier.
        .entity(EntityMutation::Update(EntityRecord::from_expanded(Arc::clone(merged)))) // clone: Arc ref-count bump; the payload is also the change event's
        .history(history)
        .build()
}

/// What one clause 5.6.2 attribute update produced, beyond the write itself.
///
/// Every field is read off the attempt that landed. The split into created,
/// updated and deleted names is a function of the payload the update was applied
/// to, so an attempt a concurrent writer superseded computed a split that
/// describes a state no client ever saw.
pub(super) struct UpdatedAttrs {
    /// The entity as it stands after the update.
    pub merged: Arc<Value>,
    /// The payload as it stood before, when a `showChanges` subscriber needs it.
    pub previous: Option<Value>,
    /// Attribute names the update added.
    pub created: Vec<AttributeName>,
    /// Attribute names the update overwrote.
    pub updated: Vec<AttributeName>,
    /// Attribute names the update removed.
    pub deleted: Vec<AttributeName>,
    /// Attribute IRIs the entity did not carry, which the handler turns into a
    /// 207 rather than a 204.
    pub not_updated: Vec<AttributeName>,
}

/// What one single-attribute mutation produced, beyond the write itself.
pub(super) struct MutatedEntity {
    /// The entity as it stands after the mutation.
    pub merged: Arc<Value>,
    /// The payload as it stood before, when a `showChanges` subscriber needs it.
    pub previous: Option<Value>,
    /// Which change the mutation describes to a subscriber.
    ///
    /// Only clause 5.6.4 varies: a null sentinel in the fragment deletes the
    /// attribute rather than updating it, and the diff is the only way to tell.
    pub trigger: NotificationTrigger,
}
