//! Everything one entity's write must land together.
//!
//! The atomic unit is one entity, because that is what the specification makes
//! atomic: clauses 5.6.1 through 5.6.6 operate on a single entity, and clauses
//! 5.6.7 through 5.6.10 return per-entity results and are explicitly permitted
//! to succeed partially. A batch is therefore N independent atomic writes, and
//! a store that can fuse them into one statement is free to — that decision
//! belongs to whoever knows the statement, not to whoever asked for the write.

use crate::service::{
    entity::{entity_mutation::EntityMutation, history_record::HistoryRecord, precondition::Precondition, side_effect::BatchSideEffectJob},
    outbox::entry::NewOutboxEntry,
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use std::sync::Arc;
use typed_builder::TypedBuilder;

/// One part of an atomic write, as a store describes what it can persist.
///
/// A store that owns every part writes them all inside its own transaction.
/// One that does not is the case section 9.1 of `docs/workspace-topology.md`
/// exists for, and the coordinator refuses the write rather than dropping the
/// part on the floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritePart {
    /// The `entities` row itself.
    Entity,
    /// The attribute history the write appends (clause 4.20).
    History,
    /// The notification outbox entries the write raises.
    Outbox,
    /// The durable side-effect job the write defers (clause 5.6.7 `Prefer: respond-async`).
    SideEffects,
}

/// Every part, in the order the coordinator applies them.
pub const ALL_WRITE_PARTS: [WritePart; 4] = [WritePart::Entity, WritePart::History, WritePart::Outbox, WritePart::SideEffects];

/// One entity's write, and everything that must commit with it.
#[derive(Debug, TypedBuilder)]
pub struct AtomicWrite {
    /// The entity this write is about.
    ///
    /// Shared rather than owned because the batch side-effect path already
    /// carries identifiers as `Arc<Uri>`, and because the identifier outlives
    /// the write in the change events it raises.
    pub entity_id: Arc<Uri>,
    /// What happens to the `entities` row.
    pub entity: EntityMutation,
    /// What the write appends to the entity's Temporal Evolution.
    #[builder(default)]
    pub history: Vec<HistoryRecord>,
    /// The notification outbox entries the write raises.
    #[builder(default)]
    pub outbox: Vec<NewOutboxEntry>,
    /// The side-effect job deferred to the durable queue, if the request opted
    /// into the asynchronous path.
    ///
    /// One job per entity even though the queue holds one row per batch: a
    /// store that fuses several writes fuses their jobs too, which keeps the
    /// row count where it was without making the atomic unit bigger than one
    /// entity.
    #[builder(default)]
    pub side_effects: Option<BatchSideEffectJob>,
    /// What the write assumes about the entity it was computed from.
    #[builder(default = Precondition::None)]
    pub precondition: Precondition,
}

impl AtomicWrite {
    /// Which parts this write actually carries.
    ///
    /// A store is only asked about the parts a write needs, so a profile with
    /// no history store can still take a write that records no history.
    pub fn parts(&self) -> Vec<WritePart> {
        let mut parts = Vec::with_capacity(ALL_WRITE_PARTS.len());
        if !self.entity.is_none() {
            parts.push(WritePart::Entity);
        }
        if !self.history.is_empty() {
            parts.push(WritePart::History);
        }
        if !self.outbox.is_empty() {
            parts.push(WritePart::Outbox);
        }
        if self.side_effects.is_some() {
            parts.push(WritePart::SideEffects);
        }
        parts
    }
}
