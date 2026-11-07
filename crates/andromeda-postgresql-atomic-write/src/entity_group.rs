//! Grouping atomic writes for fused entity statements and mapping their
//! returned identifiers back to semantic outcomes.

use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    deleted_entity::DeletedEntity,
    entity_mutation::EntityMutation,
    precondition::Precondition,
    store_instant::StoreInstant,
    write_outcome::WriteOutcome,
};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
use andromeda_postgresql_context_store::entity::{predated_entity::PredatedEntity, written_entities::WrittenEntities};
use std::collections::HashMap;

/// What a fused statement returned for the identifier a write named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowReturned<T> {
    /// The statement wrote this row, as `T` describes it.
    Landed(T),
    /// The statement refused the write, because the Entity holds the instant
    /// carried, later than the write's.
    Predated(SystemInstant),
    /// The statement did not reach this row.
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
    pub(crate) untouched: Vec<IndexedWrite<'a>>,
}

/// Group writes while preserving submission order within each statement.
pub(crate) fn group(writes: &[AtomicWrite]) -> EntityGroups<'_> {
    let mut groups = EntityGroups::default();
    for (index, write) in writes.iter().enumerate() {
        match write.entity {
            EntityMutation::Insert(_) => groups.inserts.push((index, write)),
            EntityMutation::Update(_) => groups.updates.push((index, write)),
            EntityMutation::Delete => groups.deletes.push((index, write)),
            EntityMutation::None => groups.untouched.push((index, write)),
        }
    }
    groups
}

/// Interpret a fused insert result under its semantic precondition.
pub(crate) const fn insert_outcome(precondition: Precondition, returned: RowReturned<StoreInstant>) -> WriteOutcome {
    match (returned, precondition) {
        (RowReturned::Landed(at), Precondition::None | Precondition::MustNotExist | Precondition::MustExist | Precondition::UnchangedSince(_)) => {
            WriteOutcome::Created(at)
        }
        (RowReturned::Predated(latest), Precondition::None | Precondition::MustNotExist | Precondition::MustExist | Precondition::UnchangedSince(_)) => {
            WriteOutcome::Predated(latest)
        }
        (RowReturned::No, Precondition::MustNotExist) => WriteOutcome::AlreadyExists,
        (RowReturned::No, Precondition::None | Precondition::MustExist | Precondition::UnchangedSince(_)) => WriteOutcome::Skipped,
    }
}

/// Interpret a fused update result under its semantic precondition.
pub(crate) const fn update_outcome(precondition: Precondition, returned: RowReturned<StoreInstant>) -> WriteOutcome {
    match (returned, precondition) {
        (RowReturned::Landed(at), Precondition::None | Precondition::MustNotExist | Precondition::MustExist | Precondition::UnchangedSince(_)) => {
            WriteOutcome::Updated(at)
        }
        (RowReturned::Predated(latest), Precondition::None | Precondition::MustNotExist | Precondition::MustExist | Precondition::UnchangedSince(_)) => {
            WriteOutcome::Predated(latest)
        }
        (RowReturned::No, Precondition::UnchangedSince(_)) => WriteOutcome::Superseded,
        (RowReturned::No, Precondition::None | Precondition::MustNotExist | Precondition::MustExist) => WriteOutcome::NotFound,
    }
}

/// Interpret a fused delete result under its semantic precondition.
pub(crate) fn delete_outcome(precondition: Precondition, returned: RowReturned<(DeletedEntity, StoreInstant)>) -> WriteOutcome {
    match (returned, precondition) {
        (RowReturned::Landed((entity, at)), Precondition::None | Precondition::MustNotExist | Precondition::MustExist | Precondition::UnchangedSince(_)) => {
            WriteOutcome::Deleted(entity, at)
        }
        (RowReturned::Predated(latest), Precondition::None | Precondition::MustNotExist | Precondition::MustExist | Precondition::UnchangedSince(_)) => {
            WriteOutcome::Predated(latest)
        }
        (RowReturned::No, Precondition::UnchangedSince(_)) => WriteOutcome::Superseded,
        (RowReturned::No, Precondition::None | Precondition::MustNotExist | Precondition::MustExist) => WriteOutcome::NotFound,
    }
}

/// What a fused statement returned for each Entity it reached, keyed by
/// identifier: `landed` turns a written row into its identifier and what its
/// write produced.
pub(crate) fn returned_rows<R, T>(written: WrittenEntities<R>, landed: impl Fn(R) -> (Identifier, T)) -> HashMap<Identifier, RowReturned<T>> {
    let WrittenEntities { landed: rows, predated } = written;
    rows.into_iter()
        .map(|row| {
            let (id, produced) = landed(row);
            (id, RowReturned::Landed(produced))
        })
        .chain(predated.into_iter().map(|PredatedEntity { id, latest }| (id, RowReturned::Predated(latest))))
        .collect()
}

/// What a fused statement returned for the Entity `id`, taken out of
/// `returned`; [`RowReturned::No`] when it did not reach it.
pub(crate) fn take_returned<T>(returned: &mut HashMap<Identifier, RowReturned<T>>, id: &Identifier) -> RowReturned<T> {
    returned.remove(id).unwrap_or(RowReturned::No)
}

#[cfg(test)]
mod tests {
    use crate::entity_group::{RowReturned, delete_outcome, group, insert_outcome, returned_rows, take_returned, update_outcome};
    use andromeda_context_commit::{
        atomic_write::AtomicWrite,
        entity_mutation::EntityMutation,
        entity_record::EntityRecord,
        entity_version::EntityVersion,
        precondition::Precondition,
        store_instant::StoreInstant,
        write_outcome::WriteOutcome,
    };
    use andromeda_ngsi_ld_entity::{entity_state::EntityState, entity_types::EntityTypes};
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
    use andromeda_postgresql_context_store::entity::{landed_entity::LandedEntity, predated_entity::PredatedEntity, written_entities::WrittenEntities};
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
        let committed = StoreInstant::new(Utc::now());
        pretty_assertions::assert_eq!(
            insert_outcome(Precondition::MustNotExist, RowReturned::Landed(committed)).changed_at(),
            Some(committed)
        );
        pretty_assertions::assert_eq!(
            update_outcome(Precondition::MustExist, RowReturned::Landed(committed)).changed_at(),
            Some(committed)
        );
        assert!(insert_outcome(Precondition::MustNotExist, RowReturned::No).is_already_exists());
        assert!(update_outcome(Precondition::MustExist, RowReturned::No).is_not_found());
        assert!(update_outcome(Precondition::UnchangedSince(EntityVersion::new(1, 1)), RowReturned::No).is_superseded());
        assert!(delete_outcome(Precondition::MustExist, RowReturned::No).is_not_found());
    }

    /// A refusal for a later instant answers that instant whatever the write
    /// assumed, so the caller stamps the write after it.
    #[test]
    fn a_predated_row_answers_the_entity_s_latest_instant_under_every_precondition() {
        let latest = SystemInstant::new(Utc::now());
        for precondition in [
            Precondition::None,
            Precondition::MustNotExist,
            Precondition::MustExist,
            Precondition::UnchangedSince(EntityVersion::new(1, 1)),
        ] {
            assert!(matches!(insert_outcome(precondition, RowReturned::Predated(latest)), WriteOutcome::Predated(at) if at == latest));
            assert!(matches!(update_outcome(precondition, RowReturned::Predated(latest)), WriteOutcome::Predated(at) if at == latest));
            assert!(matches!(delete_outcome(precondition, RowReturned::Predated(latest)), WriteOutcome::Predated(at) if at == latest));
        }
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
        pretty_assertions::assert_eq!(groups.untouched.iter().map(|(index, _)| *index).collect::<Vec<_>>(), vec![3]);
    }

    #[test]
    fn a_returned_row_reports_what_the_statement_did_to_its_entity() {
        let id = |text: &str| text.parse::<Identifier>().expect("valid entity URI");
        let committed = StoreInstant::new(Utc::now());
        let latest = SystemInstant::new(Utc::now());
        let written = WrittenEntities {
            landed: vec![LandedEntity {
                id: id("urn:ngsi-ld:A"),
                committed_at: committed,
            }],
            predated: vec![PredatedEntity {
                id: id("urn:ngsi-ld:B"),
                latest,
            }],
        };
        let mut returned = returned_rows(written, |entity| (entity.id, entity.committed_at));
        pretty_assertions::assert_eq!(take_returned(&mut returned, &id("urn:ngsi-ld:A")), RowReturned::Landed(committed));
        pretty_assertions::assert_eq!(take_returned(&mut returned, &id("urn:ngsi-ld:B")), RowReturned::Predated(latest));
        pretty_assertions::assert_eq!(take_returned(&mut returned, &id("urn:ngsi-ld:C")), RowReturned::No);
    }
}
