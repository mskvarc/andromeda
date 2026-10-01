//! What a deletion needs to know about the entity it removes, short of its
//! payload.
//!
//! Clause 5.6.6 removes an entity's current state. The deletion's own effect
//! and its Temporal Evolution need only the entity's types, the identity and
//! family of every attribute instance it held, and whether it held a Scope:
//! the tombstones of clauses 4.5.7 and 4.5.8 and the change clause 5.8.6 notifies are
//! stated over those. The payload itself is needed only by a subscription the
//! deletion can notify, which renders and filters the removed state, so a
//! deletion no subscription can be owed reads the outline instead of a
//! payload that may run to gigabytes once decoded.

use andromeda_context_commit::scope_presence::ScopePresence;
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_model::expanded::instance_key::AttributeInstanceKey;
use chrono::{DateTime, Utc};

/// The entity a deletion removes, as far as the deletion itself reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeletionOutline {
    /// The types the entity carried, which subscriptions select it by.
    pub types: Vec<EntityTypeName>,
    /// Every attribute instance the entity held, with its family.
    pub instances: Vec<(AttributeInstanceKey, AttributeType)>,
    /// Whether the entity held a Scope (clause 4.18).
    pub scope: ScopePresence,
    /// The entity's clause 4.8 `modifiedAt` when the outline was read, which
    /// the deletion holds against: an instance written after the read moves
    /// it, and the deletion is outlined again rather than leaving that instance
    /// without a tombstone.
    pub modified_at: DateTime<Utc>,
}

/// Whether any subscription a deletion can notify exists in the context the
/// deletion runs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeletionWatchers {
    /// No active, change-driven entity subscription listens for
    /// `entityDeleted` or `attributeDeleted`.
    None,
    /// At least one subscription may be owed a deletion's notification.
    Possible,
}
