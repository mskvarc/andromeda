//! The two column references a compiled `q` predicate is attached through.

use crate::{column_reference::ColumnReference, schema::Entities};
use sea_query::IntoIden;

/// Where a compiled `q` predicate reads the row it is filtering.
///
/// A `q` predicate is a probe: it correlates an `EXISTS` against the enclosing
/// statement's entity id, and its jsonpath fallback reads that statement's
/// payload column. Both references are supplied by the crate that owns the
/// statement rather than enumerated here, so this crate names no relation it
/// does not itself select from — a temporal statement's aliases belong to the
/// temporal crate, and a second backend's to that backend.
///
/// The references are held as the [`ColumnReference`] idens they are built from,
/// not as pre-quoted strings, so a renamed relation or column moves every
/// reference with it.
#[derive(Debug, Clone, PartialEq)]
pub struct QFilterTarget {
    /// Reference to the entity id an `EXISTS` probe correlates against.
    entity_id_column: ColumnReference,
    /// Reference to the JSONB payload column the fallback reads.
    payload_column: ColumnReference,
}

impl QFilterTarget {
    /// The target of a statement selecting directly from `entities`.
    ///
    /// Derived from the schema identifiers rather than written out, so renaming
    /// the relation or the column moves this with it.
    pub fn entities() -> QFilterTarget {
        QFilterTarget {
            entity_id_column: ColumnReference::qualified(Entities::Table, Entities::Id),
            payload_column: ColumnReference::bare(Entities::Payload),
        }
    }

    /// The target of a statement that reaches the entity through an alias.
    ///
    /// Each column is named by the `(alias, column)` idens the caller's statement
    /// already declares, so renaming a relation or a column moves this reference
    /// with it rather than leaving a hand-quoted string behind.
    pub fn correlated(entity_id_column: (impl IntoIden, impl IntoIden), payload_column: (impl IntoIden, impl IntoIden)) -> QFilterTarget {
        QFilterTarget {
            entity_id_column: ColumnReference::qualified(entity_id_column.0, entity_id_column.1),
            payload_column: ColumnReference::qualified(payload_column.0, payload_column.1),
        }
    }

    /// Quoted SQL reference to the entity id an `EXISTS` probe correlates against.
    pub fn entity_id_column(&self) -> String {
        self.entity_id_column.render()
    }

    /// Quoted SQL reference to the JSONB payload column used by the fallback.
    pub fn payload_column(&self) -> String {
        self.payload_column.render()
    }
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::target::QFilterTarget;
    use sea_query::Alias;

    #[test]
    fn the_entities_target_is_derived_from_the_schema_identifiers() {
        let target = QFilterTarget::entities();
        assert_eq!(target.entity_id_column(), "\"entities\".\"id\"");
        assert_eq!(target.payload_column(), "\"payload\"");
    }

    #[test]
    fn a_correlated_target_renders_the_quoted_reference_from_its_idens() {
        // The caller passes the `(alias, column)` idens its statement declares;
        // the quoted SQL reference is derived here rather than hand-written.
        let target = QFilterTarget::correlated((Alias::new("tea"), Alias::new("entity_id")), (Alias::new("e"), Alias::new("payload")));
        assert_eq!(target.entity_id_column(), "\"tea\".\"entity_id\"");
        assert_eq!(target.payload_column(), "\"e\".\"payload\"");
    }
}
