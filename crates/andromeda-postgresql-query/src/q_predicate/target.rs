//! The two column references a compiled `q` predicate is attached through.

use crate::schema::Entities;
use sea_query::Iden;
use std::borrow::Cow;

/// Where a compiled `q` predicate reads the row it is filtering.
///
/// A `q` predicate is a probe: it correlates an `EXISTS` against the enclosing
/// statement's entity id, and its jsonpath fallback reads that statement's
/// payload column. Both references are supplied by the crate that owns the
/// statement rather than enumerated here, so this crate names no relation it
/// does not itself select from — a temporal statement's aliases belong to the
/// temporal crate, and a second backend's to that backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QFilterTarget<'a> {
    /// SQL reference to the entity id an `EXISTS` probe correlates against.
    entity_id_column: Cow<'a, str>,
    /// SQL reference to the JSONB payload column the fallback reads.
    payload_column: Cow<'a, str>,
}

impl QFilterTarget<'_> {
    /// The target of a statement selecting directly from `entities`.
    ///
    /// Derived from the schema identifiers rather than written out, so renaming
    /// the relation or the column moves this with it.
    pub fn entities() -> QFilterTarget<'static> {
        QFilterTarget {
            entity_id_column: Cow::Owned(qualified(&Entities::Table.to_string(), &Entities::Id.to_string())),
            payload_column: Cow::Owned(quoted(&Entities::Payload.to_string())),
        }
    }

    /// SQL reference to the entity id an `EXISTS` probe correlates against.
    pub fn entity_id_column(&self) -> &str {
        &self.entity_id_column
    }

    /// SQL reference to the JSONB payload column used by the fallback.
    pub fn payload_column(&self) -> &str {
        &self.payload_column
    }
}

impl<'a> QFilterTarget<'a> {
    /// The target of a statement that reaches the entity through an alias.
    ///
    /// Both arguments are complete, already-quoted SQL references, because the
    /// caller knows which of its aliases carries them.
    pub const fn correlated(entity_id_column: &'a str, payload_column: &'a str) -> QFilterTarget<'a> {
        QFilterTarget {
            entity_id_column: Cow::Borrowed(entity_id_column),
            payload_column: Cow::Borrowed(payload_column),
        }
    }
}

/// Quote one identifier.
fn quoted(identifier: &str) -> String {
    format!("\"{identifier}\"")
}

/// Quote a `relation.column` reference.
fn qualified(relation: &str, column: &str) -> String {
    format!("\"{relation}\".\"{column}\"")
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::target::QFilterTarget;

    #[test]
    fn the_entities_target_is_derived_from_the_schema_identifiers() {
        let target = QFilterTarget::entities();
        assert_eq!(target.entity_id_column(), "\"entities\".\"id\"");
        assert_eq!(target.payload_column(), "\"payload\"");
    }

    #[test]
    fn a_correlated_target_carries_the_references_its_caller_supplied() {
        let target = QFilterTarget::correlated("\"tea\".\"entity_id\"", "\"e\".\"payload\"");
        assert_eq!(target.entity_id_column(), "\"tea\".\"entity_id\"");
        assert_eq!(target.payload_column(), "\"e\".\"payload\"");
    }
}
