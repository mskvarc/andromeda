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

use crate::{
    commit_refusal::{context_store, unit_code},
    projected_entity_key::ProjectedEntityKey,
};
use andromeda_context_commit::error::CommitError;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::expanded::attribute_expiry::instance_expires_at;
use andromeda_postgresql_codes::{attribute_kind_code::stored_attribute_kind, entity_key::EntityKey};
use andromeda_postgresql_context_store::current_attribute_values::{
    delete::delete_instances_in_tx,
    minted_insert::{MintedProjection, insert_minted_projection_in_tx},
    row::{ProjectedAttributeKey, ProjectedAttributeRow, ProjectedAttributeValue},
    upsert::upsert_projection_in_tx,
};
use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
use andromeda_postgresql_temporal_store::unit_code_metadata::unit_code_in;
use andromeda_temporal::append_intent::{TemporalAppendEntry, TemporalAppendIntent};
use sqlx::{Postgres, Transaction};
use std::collections::HashMap;

/// The projection work one commit's intents amount to.
#[derive(Debug, Default)]
struct ProjectionWork {
    /// Instances of entities the commit's own insert created, which own no row
    /// to match and are inserted outright.
    minted: MintedProjection,
    /// Instances of entities that existed before the commit, whose current
    /// value the mutation restates.
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
    target: &ContextTarget,
    intents: &[&TemporalAppendIntent],
    keys: &HashMap<Uri, ProjectedEntityKey>,
    dictionary: &HashMap<AttributeName, AttributeNameKey>,
) -> Result<(), CommitError> {
    let projected: Vec<(&TemporalAppendIntent, ProjectedEntityKey)> = intents
        .iter()
        .copied()
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
    delete_instances_in_tx(tx, target, &work.removals).await.map_err(context_store)?;
    upsert_projection_in_tx(tx, target, &work.upserts).await.map_err(context_store)?;
    insert_minted_projection_in_tx(tx, target, &work.minted).await.map_err(context_store)?;
    Ok(())
}

/// Turn one intent's entries into the projection work they describe.
fn collect(
    work: &mut ProjectionWork,
    intent: &TemporalAppendIntent,
    entity: ProjectedEntityKey,
    dictionary: &HashMap<AttributeName, AttributeNameKey>,
) -> Result<(), CommitError> {
    for entry in &intent.entries {
        match entry {
            TemporalAppendEntry::Observation { row } => {
                let key = grain(entity.key(), &row.attribute_name, row.dataset_id.as_ref(), dictionary)?;
                match ProjectedAttributeValue::try_from(&row.value) {
                    // An observation that ends the instance is a deletion in
                    // observation clothing (clauses 4.5.7 and 4.5.8): current
                    // state loses the row rather than storing a marker.
                    Err(_) => work.removals.push(key),
                    Ok(value) => {
                        let projected = ProjectedAttributeRow {
                            key,
                            attribute_kind: stored_attribute_kind(row.attribute_type),
                            value,
                            unit_code_id: unit_code_in(row.metadata.as_ref()).map_err(unit_code)?,
                            // The instance's own members other than its value
                            // are its metadata, so its clause 4.22 `expiresAt`
                            // is read from there exactly as the payload carries it.
                            expires_at: row.metadata.as_ref().and_then(instance_expires_at),
                        };
                        match entity {
                            ProjectedEntityKey::Minted(minted) => work.minted.push(minted, projected).map_err(context_store)?,
                            ProjectedEntityKey::Existing(_) => work.upserts.push(projected),
                        }
                    }
                }
            }
            TemporalAppendEntry::Tombstone {
                attribute_name, dataset_id, ..
            } => work.removals.push(grain(entity.key(), attribute_name, dataset_id.as_ref(), dictionary)?),
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
    dictionary: &HashMap<AttributeName, AttributeNameKey>,
) -> Result<ProjectedAttributeKey, CommitError> {
    let attribute_name_key = dictionary.get(attribute_name).copied().ok_or_else(|| CommitError::Invariant {
        detail: format!("attribute name dictionary returned no key for {attribute_name}"),
    })?;
    Ok(ProjectedAttributeKey {
        entity_key,
        attribute_name_key,
        dataset_id: dataset_id.cloned(), // clone: the projection grain owns its dataset identity.
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        projected_entity_key::ProjectedEntityKey,
        projection::{ProjectionWork, collect},
    };
    use andromeda_context_commit::{write_ordinal::WriteOrdinal, write_record::WriteRecord};
    use andromeda_ngsi_ld_lexicon::{
        attribute_name::AttributeName,
        attribute_type::AttributeType,
        instance_identity::InstanceIdentity,
        scope::path::ScopePath,
        uri::Uri,
    };
    use andromeda_ngsi_ld_model::temporal::{TemporalHistoryRow, TemporalValue};
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_postgresql_codes::{entity_key::EntityKey, value_kind::ValueKind};
    use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
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

    fn dictionary() -> HashMap<AttributeName, AttributeNameKey> {
        HashMap::from([(speed(), AttributeNameKey::from(7))])
    }

    /// An entity that existed before the commit, whose rows are upserted.
    fn entity_key() -> ProjectedEntityKey {
        ProjectedEntityKey::Existing(EntityKey::try_from(1).expect("a positive surrogate key"))
    }

    fn intent(entries: Vec<TemporalAppendEntry>) -> TemporalAppendIntent {
        TemporalAppendIntent {
            record: WriteRecord::mint(Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant")),
            base_ordinal: WriteOrdinal::at(0),
            entity_id: "urn:ngsi-ld:Vehicle:1".parse::<Uri>().expect("a valid entity URI"),
            entity_types: Vec::new(),
            entries,
            selection_value: None,
        }
    }

    fn observation(value: TemporalValue) -> TemporalAppendEntry {
        observation_with(value, None)
    }

    fn observation_with(value: TemporalValue, metadata: Option<serde_json::Value>) -> TemporalAppendEntry {
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
                metadata,
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
        assert!(work.removals.is_empty(), "a projectable observation removes nothing: {:?}", work.removals);
        pretty_assertions::assert_eq!(work.upserts[0].value.kind(), ValueKind::Number);
        pretty_assertions::assert_eq!(work.upserts[0].key.attribute_name_key, AttributeNameKey::from(7));
    }

    /// A String past the B-tree bound is still one upsert under the String
    /// value kind; which of the two string columns holds it is the writer's
    /// placement, not a different kind.
    #[test]
    fn a_long_string_observation_is_one_upsert_under_the_string_kind() {
        let mut work = ProjectionWork::default();
        let intent = intent(vec![observation(TemporalValue::String("x".repeat(6000)))]);
        collect(&mut work, &intent, entity_key(), &dictionary()).expect("a projectable observation");
        pretty_assertions::assert_eq!(work.upserts.len(), 1);
        pretty_assertions::assert_eq!(work.upserts[0].value.kind(), ValueKind::String);
    }

    /// Clause 4.22: past its `expiresAt` an instance is invalid, so the
    /// projection carries the instant for every read to exclude it by.
    #[test]
    fn an_observation_projects_the_instances_own_expiry() {
        let mut work = ProjectionWork::default();
        let metadata = serde_json::json!({"https://uri.etsi.org/ngsi-ld/expiresAt": [{"@value": "2030-01-01T00:00:00Z"}]});
        let intent = intent(vec![observation_with(TemporalValue::Number(JsonNumber::from(1)), Some(metadata))]);
        collect(&mut work, &intent, entity_key(), &dictionary()).expect("a projectable observation");
        pretty_assertions::assert_eq!(
            work.upserts[0].expires_at,
            Some(Utc.timestamp_opt(1_893_456_000, 0).single().expect("a representable instant"))
        );
    }

    #[test]
    fn an_observation_without_an_expiry_projects_none() {
        let mut work = ProjectionWork::default();
        let intent = intent(vec![observation(TemporalValue::Number(JsonNumber::from(1)))]);
        collect(&mut work, &intent, entity_key(), &dictionary()).expect("a projectable observation");
        pretty_assertions::assert_eq!(work.upserts[0].expires_at, None);
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
                observation: ScopeObservation::Path("/a".parse::<ScopePath>().expect("a valid scope path")),
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
        assert!(work.upserts.is_empty(), "a scope entry projects no value: {:?}", work.upserts);
        assert!(work.removals.is_empty(), "a scope entry removes nothing: {:?}", work.removals);
    }

    #[test]
    fn an_attribute_the_dictionary_never_minted_fails_the_commit() {
        let mut work = ProjectionWork::default();
        let intent = intent(vec![observation(TemporalValue::Boolean(true))]);
        assert!(collect(&mut work, &intent, entity_key(), &HashMap::new()).is_err());
    }
}
