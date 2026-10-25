//! Grouping atomic writes for fused entity statements and mapping their
//! returned identifiers back to semantic outcomes.

use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    deleted_entity::DeletedEntity,
    entity_mutation::EntityMutation,
    precondition::Precondition,
    write_outcome::WriteOutcome,
};

/// Whether a fused statement returned the identifier a write named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowReturned {
    /// The statement affected this row.
    Yes,
    /// The statement did not affect this row.
    No,
}

/// One write paired with its submission-order position.
pub(crate) type IndexedWrite<'a> = (usize, &'a AtomicWrite);

/// Writes grouped by the statement that persists their entity mutation.
#[derive(Debug, Default)]
pub(crate) struct EntityGroups<'a> {
    /// Entity inserts.
    pub(crate) inserts: Vec<IndexedWrite<'a>>,
    /// Entity updates.
    pub(crate) updates: Vec<IndexedWrite<'a>>,
    /// Entity deletes.
    pub(crate) deletes: Vec<IndexedWrite<'a>>,
    /// Writes that do not mutate current entity state.
    pub(crate) untouched: Vec<usize>,
}

/// Group writes while preserving submission order within each statement.
pub(crate) fn group(writes: &[AtomicWrite]) -> EntityGroups<'_> {
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

/// Interpret a fused insert result under its semantic precondition.
pub(crate) const fn insert_outcome(precondition: Precondition, returned: RowReturned) -> WriteOutcome {
    match (returned, precondition) {
        (RowReturned::Yes, Precondition::None | Precondition::MustNotExist | Precondition::MustExist | Precondition::UnchangedSince(_)) => {
            WriteOutcome::Created
        }
        (RowReturned::No, Precondition::MustNotExist) => WriteOutcome::AlreadyExists,
        (RowReturned::No, Precondition::None | Precondition::MustExist | Precondition::UnchangedSince(_)) => WriteOutcome::Skipped,
    }
}

/// Interpret a fused update result under its semantic precondition.
pub(crate) const fn update_outcome(precondition: Precondition, returned: RowReturned) -> WriteOutcome {
    match (returned, precondition) {
        (RowReturned::Yes, Precondition::None | Precondition::MustNotExist | Precondition::MustExist | Precondition::UnchangedSince(_)) => {
            WriteOutcome::Updated
        }
        (RowReturned::No, Precondition::UnchangedSince(_)) => WriteOutcome::Superseded,
        (RowReturned::No, Precondition::None | Precondition::MustNotExist | Precondition::MustExist) => WriteOutcome::NotFound,
    }
}

/// Interpret a fused delete result under its semantic precondition.
pub(crate) fn delete_outcome(precondition: Precondition, deleted: Option<DeletedEntity>) -> WriteOutcome {
    match (deleted, precondition) {
        (Some(entity), Precondition::None | Precondition::MustNotExist | Precondition::MustExist | Precondition::UnchangedSince(_)) => {
            WriteOutcome::Deleted(entity)
        }
        (None, Precondition::UnchangedSince(_)) => WriteOutcome::Superseded,
        (None, Precondition::None | Precondition::MustNotExist | Precondition::MustExist) => WriteOutcome::NotFound,
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::entity_group::{RowReturned, delete_outcome, group, insert_outcome, update_outcome};
    use andromeda_context_commit::{atomic_write::AtomicWrite, entity_mutation::EntityMutation, entity_record::EntityRecord, precondition::Precondition};
    use andromeda_ngsi_ld_entity::{entity_state::EntityState, entity_types::EntityTypes};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use chrono::Utc;
    use std::sync::Arc;

    fn record() -> EntityRecord {
        EntityRecord::from_state(Arc::new(EntityState::new(
            "urn:ngsi-ld:A".parse().expect("valid entity URI"),
            EntityTypes::one("https://example.org/T".parse().expect("valid type name")),
        )))
        .expect("a projectable state")
    }

    fn write(id: &str, entity: EntityMutation) -> AtomicWrite {
        AtomicWrite::builder()
            .entity_id(Arc::new(id.parse::<Identifier>().expect("valid entity URI")))
            .entity(entity)
            .build()
    }

    #[test]
    fn statement_results_map_to_precondition_outcomes() {
        assert!(insert_outcome(Precondition::MustNotExist, RowReturned::Yes).is_created());
        assert!(insert_outcome(Precondition::MustNotExist, RowReturned::No).is_already_exists());
        assert!(update_outcome(Precondition::MustExist, RowReturned::No).is_not_found());
        assert!(update_outcome(Precondition::UnchangedSince(Utc::now()), RowReturned::No).is_superseded());
        assert!(delete_outcome(Precondition::MustExist, None).is_not_found());
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
