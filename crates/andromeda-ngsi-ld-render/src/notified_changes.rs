//! What a notification reports of a change beside the state of the entity it
//! carries (clause 5.8.6).
//!
//! A notification carries the notified entity as the change left it, and,
//! depending on the change and the subscription, more than that state holds:
//!
//! - an entity deletion under `showChanges` reports every Attribute the entity
//!   held as deleted: each instance stays as it was, and its family's own value
//!   member takes the NGSI-LD Null the family is deleted by (clause 4.5.0);
//! - the Attribute instances a change deleted are reported beside the instances
//!   still held, nullified the same way, and stamped with the change's
//!   `deletedAt` when the subscription asks for system attributes (clause 4.8);
//! - under `showChanges`, every instance the change touched carries its value
//!   before the change in its family's previous member (clauses 4.5.2.2,
//!   4.5.3.2, 4.5.18.2, 4.5.20.2, 4.5.21.2, 4.5.22.2 and 4.5.24.2).
//!
//! [`NotifiedChanges`] holds the typed renderings those reports are read from,
//! and the entity walk writes them as it writes each member: nothing is copied
//! out of the entity or rewritten as a document.

use andromeda_ngsi_ld_model::entity::Entity;
use chrono::{DateTime, Utc};

/// What became of the Attributes the notified entity holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeldAttributes {
    /// They are reported as the change left them.
    Current,
    /// The entity was deleted, and every one of them is reported deleted.
    Deleted,
}

/// The Attribute instances a change deleted, as the state before it held
/// them.
#[derive(Debug, Clone, Copy)]
pub struct DeletedInstances<'a> {
    /// The rendering of the deleted instances, each under the name of the
    /// Attribute it belonged to.
    pub instances: &'a Entity,
    /// The `deletedAt` each of them is stamped with, when the subscription
    /// asks for system attributes.
    pub deleted_at: Option<DateTime<Utc>>,
}

/// Everything a notification reports beside the notified entity's state.
#[derive(Debug, Clone, Copy)]
pub struct NotifiedChanges<'a> {
    /// What became of the Attributes the entity holds.
    pub held: HeldAttributes,
    /// The instances the change deleted, if it deleted any the notification
    /// reports.
    pub deleted: Option<DeletedInstances<'a>>,
    /// Under `showChanges`, the rendering of what the change touched as it was
    /// before the change.
    pub previous: Option<&'a Entity>,
}
