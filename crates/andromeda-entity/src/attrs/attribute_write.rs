//! The shape of an attribute-level write and what it produces.
//!
//! All five operations write the entity's whole payload back and differ only in
//! how they computed it, so they share one write constructor. What they do *not*
//! share is what the merge produced for the notification, which is why there are
//! two result types rather than one bag of optional fields.

use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    entity_mutation::EntityMutation,
    entity_record::EntityRecord,
    history_record::HistoryRecord,
    subscription_match::SubscriptionMatch,
};
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::notification::NotificationTrigger;
use serde_json::Value;
use std::sync::Arc;

/// The write that stores `merged` as the entity's whole payload.
///
/// No precondition is set here:
/// [`commit_computed_write`](crate::EntityService::commit_computed_write)
/// stamps it with the `modifiedAt` of the payload the mutation was computed
/// from, which is what stops a concurrent writer's change being overwritten.
///
/// The `subscription_matches` are the durable notification records this write
/// owes, built from the attempt that actually landed so durable delivery
/// describes the change the store accepted rather than one an earlier attempt
/// computed and lost. They are empty when notification recording is not
/// configured.
pub(super) fn payload_write(id: &Uri, merged: &Arc<Value>, history: Vec<HistoryRecord>, subscription_matches: Vec<SubscriptionMatch>) -> AtomicWrite {
    AtomicWrite::builder()
        .entity_id(Arc::new(id.clone())) // clone: the write owns a shared validated identifier.
        .entity(EntityMutation::Update(EntityRecord::from_expanded(Arc::clone(merged)))) // clone: Arc ref-count bump; the payload is also the change event's
        .history(history)
        .subscription_matches(subscription_matches)
        .build()
}

/// What one single-attribute mutation produced, beyond the write itself.
pub(super) struct MutatedEntity {
    /// The entity as it stands after the mutation.
    pub merged: Arc<Value>,
    /// The entity's types after the mutation (clause 4.5.1), read off the
    /// attempt that landed so the notification carries the stored types.
    pub types: Vec<EntityTypeName>,
    /// The payload as it stood before, when a `showChanges` subscriber needs it.
    pub previous: Option<Value>,
    /// Which change the mutation describes to a subscriber.
    ///
    /// Only clause 5.6.4 varies: a null sentinel in the fragment deletes the
    /// attribute rather than updating it, and the diff is the only way to tell.
    pub trigger: NotificationTrigger,
}
