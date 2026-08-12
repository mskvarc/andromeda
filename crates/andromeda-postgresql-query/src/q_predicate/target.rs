//! The two column references a compiled `q` predicate is attached through.

use crate::{column_reference::ColumnReference, schema::Entities};
use sea_query::IntoIden;

/// Where a compiled `q` predicate reads the row it is filtering.
///
/// A `q` predicate is a probe: it correlates an `EXISTS` against the enclosing
/// statement's entity key, and its jsonpath fallback reads that statement's
/// payload column. Both references are supplied by the crate that owns the
/// statement rather than enumerated here, so this crate names no relation it
/// does not itself select from — a temporal statement's aliases belong to the
/// temporal crate, and a second backend's to that backend.
///
/// The correlation is the surrogate `entity_key`, not the Entity URI: the
/// projection an `EXISTS` probes is keyed on it, so correlating on the URI would
/// compare a `bigint` against `text`. A statement that reaches entities through
/// an alias therefore has to hand over that alias's `entity_key` column.
///
/// The references are held as the [`ColumnReference`] idens they are built from,
/// not as pre-quoted strings, so a renamed relation or column moves every
/// reference with it.
#[derive(Debug, Clone, PartialEq)]
pub struct QFilterTarget {
    /// Reference to the entity key an `EXISTS` probe correlates against.
    entity_key_column: ColumnReference,
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
            entity_key_column: ColumnReference::qualified(Entities::Table, Entities::EntityKey),
            payload_column: ColumnReference::bare(Entities::Payload),
        }
    }

    /// The target of a statement that reaches the entity through an alias.
    ///
    /// Each column is named by the `(alias, column)` idens the caller's statement
    /// already declares, so renaming a relation or a column moves this reference
    /// with it rather than leaving a hand-quoted string behind.
    pub fn correlated(entity_key_column: (impl IntoIden, impl IntoIden), payload_column: (impl IntoIden, impl IntoIden)) -> QFilterTarget {
        QFilterTarget {
            entity_key_column: ColumnReference::qualified(entity_key_column.0, entity_key_column.1),
            payload_column: ColumnReference::qualified(payload_column.0, payload_column.1),
        }
    }

    /// Quoted SQL reference to the entity key an `EXISTS` probe correlates against.
    pub fn entity_key_column(&self) -> String {
        self.entity_key_column.render()
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
    fn the_entities_target_correlates_on_the_surrogate_key_not_the_uri() {
        let target = QFilterTarget::entities();
        assert_eq!(target.entity_key_column(), "\"entities\".\"entity_key\"");
        assert_eq!(target.payload_column(), "\"payload\"");
    }

    #[test]
    fn a_correlated_target_renders_the_quoted_reference_from_its_idens() {
        // The caller passes the `(alias, column)` idens its statement declares;
        // the quoted SQL reference is derived here rather than hand-written.
        let target = QFilterTarget::correlated((Alias::new("ce"), Alias::new("entity_key")), (Alias::new("ce"), Alias::new("payload")));
        assert_eq!(target.entity_key_column(), "\"ce\".\"entity_key\"");
        assert_eq!(target.payload_column(), "\"ce\".\"payload\"");
    }
}
