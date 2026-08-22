//! The current-attribute projection phase of one atomic commit.
//!
//! `current_attribute_values` holds one row per attribute instance an entity
//! currently carries (clause 4.5.5), and it is written here, inside the same
//! transaction that writes the entity itself. Nothing publishes it afterwards
//! and nothing reconciles it later: a projection failure aborts the commit with
//! everything else, so a `q` filter can never be answered from a projection
//! that disagrees with `entity_documents.payload`.
//!
//! The interpreted intents, the surrogate keys and the attribute dictionary
//! keys all come from phases that already ran. The intents are the same ones
//! history is written from — one interpretation of the mutation serves both —
//! the entity keys are the ones the entity statements returned, so an entity
//! whose write did not land carries no key and contributes nothing here, and
//! the dictionary keys were minted from those same intents in this transaction.

use crate::commit_refusal::{persistence, unit_code};
use andromeda_context_commit::error::CommitError;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_postgresql_codes::entity_key::EntityKey;
use andromeda_postgresql_context_store::current_attribute_values::{
    delete::delete_instances_in_tx,
    row::{ProjectedAttributeKey, ProjectedAttributeRow, ProjectedAttributeValue},
    upsert::upsert_projection_in_tx,
};
use andromeda_postgresql_query::attribute_name_id::AttributeNameId;
use andromeda_postgresql_temporal_store::{attribute_kind_code::stored_attribute_kind, unit_code_metadata::unit_code_in};
use andromeda_temporal::append_intent::{TemporalAppendEntry, TemporalAppendIntent};
use sqlx::{Postgres, Transaction};
use std::collections::HashMap;

/// The projection work one commit's intents amount to.
#[derive(Debug, Default)]
struct ProjectionWork {
    /// Instances whose current value the mutation restates.
    upserts: Vec<ProjectedAttributeRow>,
    /// Instances the mutation removed from current entity state.
    removals: Vec<ProjectedAttributeKey>,
}

/// Write every landed mutation's current attribute values in the caller's
/// transaction.
///
/// # Errors
///
/// Returns [`CommitError::Persistence`] when a statement fails,
/// [`CommitError::Submission`] when an instance carries a `unitCode` the
/// catalog cannot store, and [`CommitError::Invariant`] when an observation
/// carries a value the projection has no column for or names an attribute the
/// dictionary did not resolve.
pub(crate) async fn apply(
    tx: &mut Transaction<'_, Postgres>,
    intents: &[TemporalAppendIntent],
    keys: &HashMap<Uri, EntityKey>,
    dictionary: &HashMap<AttributeName, AttributeNameId>,
) -> Result<(), CommitError> {
    let projected: Vec<(&TemporalAppendIntent, EntityKey)> = intents
        .iter()
        .filter_map(|intent| keys.get(&intent.entity_id).map(|key| (intent, *key)))
        .collect();
    if projected.is_empty() {
        return Ok(());
    }

    let mut work = ProjectionWork::default();
    for (intent, entity_key) in &projected {
        collect(&mut work, intent, *entity_key, dictionary)?;
    }
    let work = work;

    // Removals run first so that a mutation which both ends one instance and
    // restates another leaves the second standing. No grain appears on both
    // sides: an instance the mutation gives a value to is not one it removed.
    delete_instances_in_tx(tx, &work.removals).await.map_err(persistence)?;
    upsert_projection_in_tx(tx, &work.upserts).await.map_err(persistence)?;
    Ok(())
}

/// Turn one intent's entries into the projection work they describe.
fn collect(
    work: &mut ProjectionWork,
    intent: &TemporalAppendIntent,
    entity_key: EntityKey,
    dictionary: &HashMap<AttributeName, AttributeNameId>,
) -> Result<(), CommitError> {
    for entry in &intent.entries {
        match entry {
            TemporalAppendEntry::Observation { row } => {
                let key = grain(entity_key, &row.attribute_name, row.dataset_id.as_ref(), dictionary)?;
                match ProjectedAttributeValue::try_from(&row.value) {
                    // An observation that ends the instance is a deletion in
                    // observation clothing (clauses 4.5.7 and 4.5.8): current
                    // state loses the row rather than storing a marker.
                    Err(_) => work.removals.push(key),
                    Ok(value) => work.upserts.push(ProjectedAttributeRow {
                        key,
                        attribute_kind: stored_attribute_kind(row.attribute_type),
                        value,
                        unit_code_id: unit_code_in(row.metadata.as_ref()).map_err(unit_code)?,
                    }),
                }
            }
            TemporalAppendEntry::Tombstone {
                attribute_name, dataset_id, ..
            } => work.removals.push(grain(entity_key, attribute_name, dataset_id.as_ref(), dictionary)?),
            TemporalAppendEntry::Scope { .. } => {}
        }
    }
    Ok(())
}

/// The clause 4.5.5 identity of one projected instance.
fn grain(
    entity_key: EntityKey,
    attribute_name: &AttributeName,
    dataset_id: Option<&Uri>,
    dictionary: &HashMap<AttributeName, AttributeNameId>,
) -> Result<ProjectedAttributeKey, CommitError> {
    let attribute_name_id = dictionary.get(attribute_name).copied().ok_or_else(|| CommitError::Invariant {
        detail: format!("attribute name dictionary returned no key for {attribute_name}"),
    })?;
    Ok(ProjectedAttributeKey {
        entity_key,
        attribute_name_id,
        dataset_id: dataset_id.cloned(), // clone: the projection grain owns its dataset identity.
    })
}

#[cfg(test)]
mod tests {
    use crate::projection::{ProjectionWork, collect};
    use andromeda_context_commit::{write_ordinal::WriteOrdinal, write_record::WriteRecord};
    use andromeda_ngsi_ld_lexicon::{
        attribute_name::AttributeName,
        attribute_type::AttributeType,
        instance_identity::InstanceIdentity,
        scope::grammar::ScopeBuf,
        uri::Uri,
    };
    use andromeda_ngsi_ld_model::temporal::{TemporalHistoryRow, TemporalValue};
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_postgresql_codes::{entity_key::EntityKey, value_kind::ValueKind};
    use andromeda_postgresql_query::attribute_name_id::AttributeNameId;
    use andromeda_temporal::{
        append_intent::{TemporalAppendEntry, TemporalAppendIntent},
        scope_observation::ScopeObservation,
    };
    use chrono::{TimeZone, Utc};
    use std::collections::HashMap;
    use uuid::Uuid;

    fn speed() -> AttributeName {
        "https://example.com/speed".parse().expect("a valid attribute name")
    }

    fn dictionary() -> HashMap<AttributeName, AttributeNameId> {
        HashMap::from([(speed(), AttributeNameId::from(7))])
    }

    fn entity_key() -> EntityKey {
        EntityKey::try_from(1).expect("a positive surrogate key")
    }

    fn intent(entries: Vec<TemporalAppendEntry>) -> TemporalAppendIntent {
        TemporalAppendIntent {
            record: WriteRecord::mint(Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant")),
            base: WriteOrdinal::at(0),
            entity_id: "urn:ngsi-ld:Vehicle:1".parse::<Uri>().expect("a valid entity URI"),
            entries,
            selection_value: None,
        }
    }

    fn observation(value: TemporalValue) -> TemporalAppendEntry {
        TemporalAppendEntry::Observation {
            row: TemporalHistoryRow {
                attribute_name: speed(),
                attribute_type: AttributeType::Property,
                dataset_id: None,
                value,
                observed_at: None,
                recorded_at: Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant"),
                created_at: None,
                modified_at: None,
                instance: InstanceIdentity::Generated(Uuid::nil()),
                metadata: None,
                partition_total: None,
            },
        }
    }

    #[test]
    fn an_observation_becomes_one_upsert_at_the_instance_grain() {
        let mut work = ProjectionWork::default();
        let intent = intent(vec![observation(TemporalValue::Number(JsonNumber::from(42)))]);
        collect(&mut work, &intent, entity_key(), &dictionary()).expect("a projectable observation");
        pretty_assertions::assert_eq!(work.upserts.len(), 1);
        assert!(work.removals.is_empty());
        pretty_assertions::assert_eq!(work.upserts[0].value.kind(), ValueKind::Number);
        pretty_assertions::assert_eq!(work.upserts[0].key.attribute_name_id, AttributeNameId::from(7));
    }

    #[test]
    fn an_observation_that_ends_the_instance_removes_its_row_instead_of_marking_it() {
        let deleted_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        let mut work = ProjectionWork::default();
        let intent = intent(vec![observation(TemporalValue::Tombstone { deleted_at })]);
        collect(&mut work, &intent, entity_key(), &dictionary()).expect("a projectable observation");
        assert!(work.upserts.is_empty(), "no projected row may declare a deletion");
        pretty_assertions::assert_eq!(work.removals.len(), 1);
    }

    #[test]
    fn a_tombstone_entry_removes_the_instance_it_names() {
        let mut work = ProjectionWork::default();
        let intent = intent(vec![TemporalAppendEntry::Tombstone {
            attribute_name: speed(),
            dataset_id: Some("urn:ngsi-ld:Dataset:a".parse().expect("a valid dataset URI")),
            attribute_type: AttributeType::Relationship,
            instance: InstanceIdentity::Generated(Uuid::nil()),
        }]);
        collect(&mut work, &intent, entity_key(), &dictionary()).expect("a resolvable tombstone");
        pretty_assertions::assert_eq!(work.removals.len(), 1);
        pretty_assertions::assert_eq!(work.removals[0].dataset_id.as_ref().map(Uri::as_str), Some("urn:ngsi-ld:Dataset:a"));
    }

    #[test]
    fn a_scope_entry_contributes_no_projection_work() {
        let mut work = ProjectionWork::default();
        let entries = vec![
            TemporalAppendEntry::Scope {
                observation: ScopeObservation::Path(ScopeBuf::new("/a".to_owned()).expect("a valid scope")),
                observed_at: None,
                instance: InstanceIdentity::Generated(Uuid::nil()),
            },
            TemporalAppendEntry::Scope {
                observation: ScopeObservation::Deleted,
                observed_at: None,
                instance: InstanceIdentity::Generated(Uuid::nil()),
            },
        ];
        collect(&mut work, &intent(entries), entity_key(), &dictionary()).expect("a scope entry");
        assert!(work.upserts.is_empty());
        assert!(work.removals.is_empty());
    }

    #[test]
    fn an_attribute_the_dictionary_never_minted_fails_the_commit() {
        let mut work = ProjectionWork::default();
        let intent = intent(vec![observation(TemporalValue::Boolean(true))]);
        assert!(collect(&mut work, &intent, entity_key(), &HashMap::new()).is_err());
    }
}
