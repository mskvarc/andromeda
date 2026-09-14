//! The narrow key a temporal entity is referenced by.

use derive_more::Display;
use thiserror::Error;

/// The surrogate key of one temporal entity.
///
/// Temporal history is the widest thing the broker stores, so the column that
/// repeats on every stream and every scope row is a generated eight-byte
/// identity rather than the entity's own identifier text. The key is assigned
/// once by the database and never reassigned: a reused number would attach one
/// entity's recorded history to another.
///
/// The temporal catalogue is deliberately not keyed by, or constrained to, the
/// current-state entity table. Deleting an entity (clause 5.6.6) leaves its
/// temporal evolution standing, and only Temporal Evolution deletion (clause
/// 5.6.16) removes it, so a temporal key outliving a current-state row is
/// correct rather than dangling.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[display("{_0}")]
pub struct TemporalEntityKey(i64);

impl TemporalEntityKey {
    /// Return the stored representation.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl TryFrom<i64> for TemporalEntityKey {
    type Error = TemporalEntityKeyOutOfRange;

    fn try_from(stored: i64) -> Result<TemporalEntityKey, TemporalEntityKeyOutOfRange> {
        if stored > 0 {
            Ok(TemporalEntityKey(stored))
        } else {
            Err(TemporalEntityKeyOutOfRange)
        }
    }
}

/// A temporal-entity key was outside the range the database assigns.
#[derive(Clone, Copy, Debug, Eq, Error, Hash, PartialEq)]
#[error("a temporal-entity key is a positive number")]
pub struct TemporalEntityKeyOutOfRange;

#[cfg(test)]
mod tests {
    use crate::temporal_entity_key::{TemporalEntityKey, TemporalEntityKeyOutOfRange};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_stored_key_round_trips() {
        assert_eq!(TemporalEntityKey::try_from(4_294_967_296_i64).map(TemporalEntityKey::get), Ok(4_294_967_296));
    }

    #[test]
    fn zero_and_negative_keys_are_refused() {
        assert_eq!(TemporalEntityKey::try_from(0), Err(TemporalEntityKeyOutOfRange));
        assert_eq!(TemporalEntityKey::try_from(-1), Err(TemporalEntityKeyOutOfRange));
    }

    #[test]
    fn a_key_renders_as_the_number_it_stores() {
        assert_eq!(TemporalEntityKey::try_from(17).map(|key| key.to_string()), Ok("17".to_owned()));
    }
}
