//! The version token a compare-and-set entity write states.

/// The version an entity was read at: the key of the stored row that holds it
/// and the count of writes that row has taken.
///
/// Every write of an entity's current state moves the count, so two writes of
/// the same entity never leave the same version behind, whatever instant each
/// stamped. The row key tells a re-created entity apart from the one a hard
/// deletion removed under the same identifier, whose count started over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityVersion {
    /// The surrogate key of the stored row, never reused for another entity.
    entity_key: i64,
    /// How many writes the row has taken, starting at one.
    write_count: i64,
}

impl EntityVersion {
    /// The version of the row keyed `entity_key` after `write_count` writes.
    #[must_use]
    pub const fn new(entity_key: i64, write_count: i64) -> EntityVersion {
        EntityVersion { entity_key, write_count }
    }

    /// The surrogate key of the stored row.
    #[must_use]
    pub const fn entity_key(self) -> i64 {
        self.entity_key
    }

    /// How many writes the row has taken.
    #[must_use]
    pub const fn write_count(self) -> i64 {
        self.write_count
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_version::EntityVersion;

    #[test]
    fn a_later_write_of_the_same_row_is_another_version() {
        assert_ne!(EntityVersion::new(7, 1), EntityVersion::new(7, 2));
    }

    /// A re-created entity starts its count over on a new row.
    #[test]
    fn the_same_count_on_another_row_is_another_version() {
        assert_ne!(EntityVersion::new(7, 1), EntityVersion::new(8, 1));
    }
}
