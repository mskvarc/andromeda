//! The narrow key a current-state entity is referenced by.

use derive_more::Display;
use thiserror::Error;

/// The surrogate key of one current-state entity.
///
/// Every relation owned by an entity repeats this column on each of its rows,
/// so the current-state grain is carried by a generated eight-byte identity
/// rather than by the entity's own identifier text. The key is assigned once by
/// the database and never reassigned: a reused number would attach one entity's
/// projected attribute values to another.
///
/// The identifier a client supplies stays the entity's external identity —
/// clause 5.6.x addresses an entity by its URI, and this key is never rendered
/// or accepted over the API. It exists only below the storage boundary, where
/// owned child relations join it and cascade from it.
///
/// It is deliberately unrelated to the temporal catalogue's own entity key.
/// Core Entity deletion (clause 5.6.6) removes the current-state row while the
/// temporal evolution survives until Temporal Evolution deletion (clause
/// 5.6.16), so the two grains carry independent identities and are never joined.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[display("{_0}")]
pub struct EntityKey(i64);

impl EntityKey {
    /// Return the stored representation.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl TryFrom<i64> for EntityKey {
    type Error = EntityKeyOutOfRange;

    fn try_from(stored: i64) -> Result<EntityKey, EntityKeyOutOfRange> {
        if stored > 0 { Ok(EntityKey(stored)) } else { Err(EntityKeyOutOfRange) }
    }
}

/// An entity key was outside the range the database assigns.
#[derive(Clone, Copy, Debug, Eq, Error, Hash, PartialEq)]
#[error("an entity key is a positive number")]
pub struct EntityKeyOutOfRange;

#[cfg(test)]
mod tests {
    use crate::entity_key::{EntityKey, EntityKeyOutOfRange};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_stored_key_round_trips() {
        assert_eq!(EntityKey::try_from(4_294_967_296_i64).map(EntityKey::get), Ok(4_294_967_296));
    }

    #[test]
    fn zero_and_negative_keys_are_refused() {
        assert_eq!(EntityKey::try_from(0), Err(EntityKeyOutOfRange));
        assert_eq!(EntityKey::try_from(-1), Err(EntityKeyOutOfRange));
    }

    #[test]
    fn a_key_renders_as_the_number_it_stores() {
        assert_eq!(EntityKey::try_from(17).map(|key| key.to_string()), Ok("17".to_owned()));
    }
}
