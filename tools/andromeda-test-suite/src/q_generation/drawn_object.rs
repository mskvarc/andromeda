//! The object of a drawn Relationship or `ListRelationship`: a URI of the pool, or an Entity of the case.
//!
//! An object naming an Entity of the case names one at a lower position. Each round of writes goes in position order,
//! so the Linked Entity a hop reads is written before the Entity naming it and is in its final state for that round
//! whether the Subscription reads it at the write or Query Entities reads it after the round.

use crate::q_generation::{drawn_pool::OBJECTS, placeholder};
use googletest::prelude::*;
use ngsi_ld::model::EntityId;
use proptest::{
    prelude::{BoxedStrategy, Strategy, prop_oneof},
    sample::select,
};

/// One object of a drawn Relationship or `ListRelationship`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawnObject {
    /// A URI of the pool, which identifies no Entity of the case.
    Fixed(&'static str),
    /// The Entity at this position of the case, lower than the position of the Entity naming it.
    Earlier(usize),
}

impl DrawnObject {
    /// The object as an Entity identifier, `ids` identifying the Entity at each position.
    ///
    /// # Errors
    ///
    /// Fails when a URI of the pool is not an identifier, or `ids` has no identifier at the position.
    pub fn id(self, ids: &[EntityId]) -> Result<EntityId> {
        Ok(match self {
            DrawnObject::Fixed(uri) => uri.parse()?,
            DrawnObject::Earlier(position) => ids.get(position).or_fail()?.clone(),
        })
    }

    /// The object as an unquoted `q` operand, an Entity of the case as its placeholder.
    #[must_use]
    pub fn operand(self) -> String {
        match self {
            DrawnObject::Fixed(uri) => uri.to_owned(),
            DrawnObject::Earlier(position) => placeholder::entity(position),
        }
    }

    /// The position of the Entity of the case it names, None for a URI of the pool.
    #[must_use]
    pub const fn position(self) -> Option<usize> {
        match self {
            DrawnObject::Fixed(_) => None,
            DrawnObject::Earlier(position) => Some(position),
        }
    }
}

/// An object of a Relationship of the Entity at `position`: a URI of the pool, or for every Entity but the first, as
/// often an Entity at a lower position.
pub fn drawn_object(position: usize) -> BoxedStrategy<DrawnObject> {
    let fixed = select(&OBJECTS[..]).prop_map(DrawnObject::Fixed);
    if position == 0 {
        return fixed.boxed();
    }
    prop_oneof![fixed, (0..position).prop_map(DrawnObject::Earlier)].boxed()
}
