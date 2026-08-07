//! Sorting atomic writes into the fused statements that carry them, and
//! reading each write's outcome back out of what the statement returned.
//!
//! Four statements at most serve any number of writes: one insert, one update,
//! one delete, and nothing at all for the writes that touch no row. Which
//! outcome a returned -- or missing -- identifier means depends on what the
//! write assumed, which is why the mapping lives here as plain functions rather
//! than inside the statement calls.

use crate::service::entity::{
    atomic_write::AtomicWrite,
    entity_mutation::EntityMutation,
    precondition::Precondition,
    store::DeletedEntity,
    write_outcome::WriteOutcome,
};

/// Whether a fused statement returned the identifier a write named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RowReturned {
    /// The statement affected this write's row.
    Yes,
    /// It did not.
    No,
}

/// One write together with its position in the caller's submission order.
pub(super) type IndexedWrite<'a> = (usize, &'a AtomicWrite);

/// Writes sorted by the statement that will carry them.
#[derive(Debug, Default)]
pub(super) struct EntityGroups<'a> {
    /// Writes that add a row.
    pub inserts: Vec<IndexedWrite<'a>>,
    /// Writes that overwrite a row.
    pub updates: Vec<IndexedWrite<'a>>,
    /// Writes that remove a row.
    pub deletes: Vec<IndexedWrite<'a>>,
    /// Positions of writes that touch no row at all.
    pub untouched: Vec<usize>,
}

/// Sort `writes` by the statement each one belongs to, preserving submission
/// order within every group.
pub(super) fn group(writes: &[AtomicWrite]) -> EntityGroups<'_> {
    let mut groups = EntityGroups::default();
    for (index, write) in writes.iter().enumerate() {
        match write.entity {
            EntityMutation::Insert(_) => groups.inserts.push((index, write)),
            EntityMutation::Update(_) => groups.updates.push((index, write)),
            EntityMutation::Delete => groups.deletes.push((index, write)),
            EntityMutation::None => groups.untouched.push(index),
        }
    }
    groups
}

/// What an insert means when its identifier did or did not come back.
///
/// A conflicting insert is only a failure when the write said the entity must
/// not already exist; the temporal placeholder and the create half of an
/// upsert both want the existing row left alone (NGSI-LD 6.4.3.1, 5.6.8).
pub(super) const fn insert_outcome(precondition: Precondition, returned: RowReturned) -> WriteOutcome {
    match (returned, precondition) {
        (RowReturned::Yes, Precondition::None | Precondition::MustNotExist | Precondition::MustExist | Precondition::UnchangedSince(_)) => {
            WriteOutcome::Created
        }
        (RowReturned::No, Precondition::MustNotExist) => WriteOutcome::AlreadyExists,
        (RowReturned::No, Precondition::None | Precondition::MustExist | Precondition::UnchangedSince(_)) => WriteOutcome::Skipped,
    }
}

/// What an update means when its identifier did or did not come back.
///
/// A missing identifier under `UnchangedSince` is reported as superseded rather
/// than absent: one statement cannot tell "the row moved on" from "the row is
/// gone", and the caller's re-read distinguishes them for free.
pub(super) const fn update_outcome(precondition: Precondition, returned: RowReturned) -> WriteOutcome {
    match (returned, precondition) {
        (RowReturned::Yes, Precondition::None | Precondition::MustNotExist | Precondition::MustExist | Precondition::UnchangedSince(_)) => {
            WriteOutcome::Updated
        }
        (RowReturned::No, Precondition::UnchangedSince(_)) => WriteOutcome::Superseded,
        (RowReturned::No, Precondition::None | Precondition::MustNotExist | Precondition::MustExist) => WriteOutcome::NotFound,
    }
}

/// What a delete means when its row did or did not come back.
///
/// Reads the same way [`update_outcome`] does, and for the same reason: a delete
/// computed from a payload the caller read outside the transaction is a
/// read-modify-write, so a missing row under `UnchangedSince` means the entity
/// moved on rather than that it was never there.
pub(super) fn delete_outcome(precondition: Precondition, deleted: Option<DeletedEntity>) -> WriteOutcome {
    match (deleted, precondition) {
        (Some(entity), Precondition::None | Precondition::MustNotExist | Precondition::MustExist | Precondition::UnchangedSince(_)) => {
            WriteOutcome::Deleted(entity)
        }
        (None, Precondition::UnchangedSince(_)) => WriteOutcome::Superseded,
        (None, Precondition::None | Precondition::MustNotExist | Precondition::MustExist) => WriteOutcome::NotFound,
    }
}

#[cfg(test)]
mod tests {
    use super::{RowReturned, delete_outcome, group, insert_outcome, update_outcome};
    use crate::service::entity::{
        atomic_write::AtomicWrite,
        entity_mutation::{EntityMutation, EntityRecord},
        precondition::Precondition,
        store::DeletedEntity,
    };
    use chrono::Utc;
    use serde_json::json;
    use std::sync::Arc;

    fn record() -> EntityRecord {
        EntityRecord {
            payload: Arc::new(json!([{"@id": "urn:ngsi-ld:A"}])),
            types: Vec::new(),
            scopes: Vec::new(),
            location: None,
            observation_space: None,
            operation_space: None,
            expires_at: None,
        }
    }

    fn write(id: &str, entity: EntityMutation) -> AtomicWrite {
        AtomicWrite::builder().entity_id(Arc::from(id)).entity(entity).build()
    }

    #[test]
    fn an_insert_whose_identifier_returns_is_created() {
        assert!(insert_outcome(Precondition::MustNotExist, RowReturned::Yes).is_created());
    }

    #[test]
    fn an_insert_whose_identifier_is_absent_already_exists() {
        assert!(insert_outcome(Precondition::MustNotExist, RowReturned::No).is_already_exists());
    }

    #[test]
    fn an_unconditional_insert_whose_identifier_is_absent_is_skipped() {
        assert!(insert_outcome(Precondition::None, RowReturned::No).is_skipped());
    }

    #[test]
    fn an_update_whose_identifier_is_absent_is_not_found() {
        assert!(update_outcome(Precondition::MustExist, RowReturned::No).is_not_found());
    }

    #[test]
    fn an_expectant_update_whose_identifier_is_absent_is_superseded() {
        assert!(update_outcome(Precondition::UnchangedSince(Utc::now()), RowReturned::No).is_superseded());
    }

    #[test]
    fn an_update_whose_identifier_returns_is_updated() {
        assert!(update_outcome(Precondition::UnchangedSince(Utc::now()), RowReturned::Yes).is_updated());
        assert!(update_outcome(Precondition::MustExist, RowReturned::Yes).is_updated());
    }

    fn deleted_entity() -> DeletedEntity {
        let now = Utc::now();
        DeletedEntity {
            id: "urn:ngsi-ld:A".parse().expect("valid uri"),
            payload: json!([{"@id": "urn:ngsi-ld:A"}]),
            created_at: now,
            modified_at: now,
            expires_at: None,
        }
    }

    #[test]
    fn a_delete_whose_row_returns_reports_what_it_held() {
        assert!(delete_outcome(Precondition::MustExist, Some(deleted_entity())).is_deleted());
    }

    #[test]
    fn a_delete_whose_row_is_absent_is_not_found() {
        assert!(delete_outcome(Precondition::MustExist, None).is_not_found());
    }

    #[test]
    fn an_expectant_delete_whose_row_is_absent_is_superseded() {
        // The row may be gone or may have moved on; one statement cannot tell,
        // and the caller's re-read distinguishes them for free.
        assert!(delete_outcome(Precondition::UnchangedSince(Utc::now()), None).is_superseded());
    }

    #[test]
    fn grouping_preserves_submission_order_within_each_statement() {
        let writes = vec![
            write("urn:ngsi-ld:A", EntityMutation::Insert(record())),
            write("urn:ngsi-ld:B", EntityMutation::Delete),
            write("urn:ngsi-ld:C", EntityMutation::Update(record())),
            write("urn:ngsi-ld:D", EntityMutation::None),
            write("urn:ngsi-ld:E", EntityMutation::Insert(record())),
        ];

        let groups = group(&writes);

        pretty_assertions::assert_eq!(groups.inserts.iter().map(|(index, _)| *index).collect::<Vec<_>>(), vec![0, 4]);
        pretty_assertions::assert_eq!(groups.updates.iter().map(|(index, _)| *index).collect::<Vec<_>>(), vec![2]);
        pretty_assertions::assert_eq!(groups.deletes.iter().map(|(index, _)| *index).collect::<Vec<_>>(), vec![1]);
        pretty_assertions::assert_eq!(groups.untouched, vec![3]);
    }
}
