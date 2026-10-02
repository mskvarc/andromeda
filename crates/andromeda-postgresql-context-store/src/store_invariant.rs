//! Conditions the context store's own callers guarantee, and what breaking one
//! looks like.
//!
//! None of these is reachable from client input: each names a contract between
//! two pieces of this adapter, or an identifier it minted itself, and reaching
//! one means the adapter is wrong rather than the data or the database.

use andromeda_postgresql_codes::entity_key::EntityKey;
use strum::Display;
use thiserror::Error;

/// A guarded entity write that takes one `modifiedAt` expectation per row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
#[strum(serialize_all = "kebab-case")]
pub enum GuardedEntityWrite {
    /// The optimistic update of existing entities.
    Update,
    /// The optimistic delete of existing entities.
    Delete,
}

/// A contract inside the context store that its own caller broke.
#[derive(Debug, Error)]
pub enum StoreInvariant {
    /// A `current_attribute_values` projection row was offered to the insert
    /// of an entity it does not belong to, which would let a row of an existing
    /// entity bypass the upsert.
    #[error("projection row of entity {row} offered under minted key {minted}")]
    ProjectionRowOfAnotherEntity {
        /// The entity the row is keyed to.
        row: EntityKey,
        /// The freshly minted entity the insert was offered for.
        minted: EntityKey,
    },
    /// A guarded write was handed a different number of `modifiedAt`
    /// expectations than rows to write.
    #[error("entity {write} given {expectations} expectations for {rows} rows")]
    ExpectationCount {
        /// Which guarded write was called.
        write: GuardedEntityWrite,
        /// How many expectations it was handed.
        expectations: usize,
        /// How many rows it was asked to write.
        rows: usize,
    },
    /// A statement that creates search rows was handed a row that keeps its
    /// stored geometry, which a row being created does not have.
    #[error("a created search row was staged to keep a geometry it does not hold")]
    KeptGeometryWithoutStoredRow,
}

#[cfg(test)]
mod tests {
    use crate::store_invariant::{GuardedEntityWrite, StoreInvariant};
    use andromeda_postgresql_codes::entity_key::EntityKey;

    #[test]
    fn an_expectation_count_mismatch_names_the_guarded_write_and_both_counts() {
        let invariant = StoreInvariant::ExpectationCount {
            write: GuardedEntityWrite::Delete,
            expectations: 2,
            rows: 3,
        };
        pretty_assertions::assert_eq!(invariant.to_string(), "entity delete given 2 expectations for 3 rows");
    }

    #[test]
    fn a_projection_row_of_another_entity_names_both_keys() {
        let (Ok(row), Ok(minted)) = (EntityKey::try_from(7_i64), EntityKey::try_from(9_i64)) else {
            panic!("positive keys are valid entity keys");
        };
        let invariant = StoreInvariant::ProjectionRowOfAnotherEntity { row, minted };
        pretty_assertions::assert_eq!(invariant.to_string(), "projection row of entity 7 offered under minted key 9");
    }
}
