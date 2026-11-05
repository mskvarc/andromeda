//! What one batch create landed, and which Temporal Evolutions it wrote
//! beside the current state it created.

use crate::entity::{landed_entity::LandedEntity, predated_entity::PredatedEntity};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// The Entities one batch create landed.
#[derive(Debug, Default)]
pub struct CreatedEntities {
    /// Every Entity the create gave a current state: a new identity row, or
    /// one that held only a Temporal Evolution (clause 5.6.1 creates it anew).
    pub landed: Vec<LandedEntity>,
    /// The landed Entities whose identity row the create minted with the
    /// Temporal Evolution it was asked to establish: created and modified at
    /// the create's instant, under the Entity's types, with every type
    /// membership on both sides. An identity row that existed keeps the
    /// evolution it held, so none of those is here.
    pub established: Vec<Identifier>,
    /// The identity rows holding no current state whose creation
    /// the create refused, because they hold an instant later than its own
    /// (clause 4.8), with that instant.
    pub predated: Vec<PredatedEntity>,
}
