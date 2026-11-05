//! What one update or delete statement wrote, and which of its writes it
//! refused as predated.

use crate::entity::predated_entity::PredatedEntity;

/// The Entities one statement wrote, as `T` describes each, and those whose
/// write it refused because they hold a later instant.
#[derive(Debug)]
pub struct WrittenEntities<T> {
    /// Every Entity the statement wrote.
    pub landed: Vec<T>,
    /// Every Entity whose write the statement refused, with the latest instant
    /// it holds.
    pub predated: Vec<PredatedEntity>,
}

impl<T> WrittenEntities<T> {
    /// A statement that wrote nothing and refused nothing.
    #[must_use]
    pub const fn none() -> WrittenEntities<T> {
        WrittenEntities {
            landed: Vec::new(),
            predated: Vec::new(),
        }
    }
}
