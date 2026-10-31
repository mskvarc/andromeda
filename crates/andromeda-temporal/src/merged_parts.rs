//! The parts an entity held in more than one place is aggregated over at this broker (clause 5.7.4.4).
//!
//! Clause 5.7.4.4 renders the "aggregated values of Attribute instances" of the entity merged into S4. The parts the sources answered are always in it. This broker's own part is in it only when the local selection kept the entity: without split entities the value, geo and scope filters select S2 to S4 from the local entities before anything is merged, so a local part that failed them is no part of the merged entity, while with split entities every part is merged first and the filters are decided over the whole.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::temporal_document::TemporalDocument;

/// Whether this broker's own part of an entity is in the entity merged into S4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalPart {
    /// The local instances of the entity are aggregated with the answered ones.
    Included,
    /// The local instances are left out: the local selection did not keep the entity, so only the answered parts make it.
    Excluded,
}

/// The parts one entity is aggregated over.
#[derive(Debug)]
pub struct EntityParts {
    /// The entity.
    pub entity_id: Identifier,
    /// Whether this broker's own instances of it join the aggregation.
    pub local: LocalPart,
    /// The documents the sources answered for it, in the order they came.
    pub answered: Vec<TemporalDocument>,
}
