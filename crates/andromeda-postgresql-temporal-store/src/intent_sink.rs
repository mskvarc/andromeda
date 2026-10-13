//! Recording a commit's temporal append intents as history rows in the
//! commit's own transaction.

use crate::{
    attribute_history_row::AttributeHistoryRow,
    attribute_history_value::AttributeHistoryValue,
    error::TemporalStoreError,
    history_members::{HistoryMembers, mention},
    repo::TemporalRepository,
    scope_history_entry::ScopeHistoryEntry,
    scope_history_value::ScopeHistoryValue,
    semantic_time_axes::SemanticTimeAxes,
    temporal_catalog_write::TemporalCatalog,
    temporal_store_invariant::TemporalStoreInvariant,
    temporal_stream::{TemporalCatalogUpsert, TemporalStreamKey},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::temporal::{TemporalHistoryRow, observation_metadata::ObservationMetadata};
use andromeda_postgresql_attribute_instance::{name_keys::MentionedNames, name_resolution::resolve_mentioned};
use andromeda_postgresql_codes::{attribute_kind_code::stored_attribute_kind, stream_key::StreamKey, temporal_entity_key::TemporalEntityKey};
use andromeda_temporal::{
    append_intent::{TemporalAppendEntry, TemporalAppendIntent, TemporalObservationIdentity},
    scope_observation::ScopeObservation,
    write_purpose::TemporalWritePurpose,
};
use chrono::{DateTime, Utc};

/// Append a whole chunk of typed intents in the caller's transaction.
///
/// The catalogue is resolved once for the chunk — every entity's narrow key
/// and every grain's stream in a fixed number of statements regardless of
/// how many entities the chunk carries — then the history and scope rows of
/// every intent are built in memory against that one catalogue and landed in
/// one history insert and one scope insert. Per-intent ordinal derivation
/// stays per-intent; only the statements are hoisted to the chunk.
///
/// An intent that only ends instances is passed over when the entity has no
/// Temporal Evolution left: clause 5.6.16 deletes one independently of
/// clause 5.6.6's deletion of current state, so a client can delete history
/// first and there is then no sequence for clauses 4.5.7 and 4.5.8 to record
/// an ending in.
///
/// # Errors
///
/// Returns [`TemporalStoreError::NotFound`] when an intent records an
/// observation against a Temporal Evolution that does not exist and declares
/// no entity type to create it with, [`TemporalStoreError::Validation`] when
/// an entry cannot be encoded, and [`TemporalStoreError::Database`] when a
/// statement fails.
pub(crate) async fn append_intents_in_tx(
    repository: &TemporalRepository,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    target: &ContextTarget,
    intents: &[TemporalAppendIntent],
) -> Result<(), TemporalStoreError> {
    if intents.is_empty() {
        return Ok(());
    }
    let upserts: Vec<TemporalCatalogUpsert> = intents.iter().map(catalog_upsert).collect();
    // Every intent in the chunk shares its commit's one ingestion instant, so
    // the catalogue's write clock is that instant.
    let recorded_at = intents[0].record.recorded_at();
    let catalog = repository.upsert_temporal_catalog_batch_in_tx(tx, target, &upserts, recorded_at).await?;
    let mut rows = IntentRows::default();
    for intent in intents {
        // Every failure below is this intent's own, so it names the entity
        // whose write the commit can set aside when it is a refusal.
        rows.extend(&catalog, intent).map_err(|source| TemporalStoreError::EntityIntent {
            entity_id: intent.entity_id.clone(), // clone: the failure owns the entity it names.
            source: Box::new(source),
        })?;
    }
    let IntentRows { history, scopes } = rows;
    let mut mentioned = MentionedNames::default();
    history
        .iter()
        .filter_map(|(_, metadata)| *metadata)
        .for_each(|metadata| mention(metadata, &mut mentioned));
    let keys = resolve_mentioned(tx, mentioned, repository.entity_type_names.as_ref(), repository.attribute_names.as_ref()).await?;
    let history = history
        .into_iter()
        .map(|(mut row, metadata)| {
            if let Some(metadata) = metadata {
                row.members = HistoryMembers::keyed(metadata, &keys)?;
            }
            Ok(row)
        })
        .collect::<Result<Vec<_>, TemporalStoreError>>()?;
    repository.insert_history_batch_in_tx(tx, target, history).await?;
    repository.insert_scope_history_batch_in_tx(tx, target, &scopes).await?;
    Ok(())
}

/// The history and scope rows a chunk's intents amount to.
#[derive(Default)]
struct IntentRows<'a> {
    /// Attribute history rows, observations and endings alike, each
    /// observation beside the metadata its members are keyed from once the
    /// chunk's names are minted.
    history: Vec<(AttributeHistoryRow, Option<&'a ObservationMetadata>)>,
    /// Scope history rows.
    scopes: Vec<ScopeHistoryEntry>,
}

impl<'a> IntentRows<'a> {
    /// Add the rows one intent records, resolved against the chunk's catalogue.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError`] when an entry cannot be encoded, or when
    /// the catalogue resolved no key or stream the intent needs.
    fn extend(&mut self, catalog: &TemporalCatalog, intent: &'a TemporalAppendIntent) -> Result<(), TemporalStoreError> {
        let recorded_at = intent.record.recorded_at();
        let Some(entity_key) = recording_entity_key(catalog, intent)? else {
            return Ok(());
        };
        for (position, entry) in intent.entries.iter().enumerate() {
            let delta = u32::try_from(position).map_err(TemporalStoreError::EntryPosition)?;
            let ordinal = intent.base_ordinal.advanced_by(delta).map_err(TemporalStoreError::WriteOrdinal)?;
            let identity = TemporalObservationIdentity {
                write_id: intent.record.write_id(),
                ordinal,
            };
            match entry {
                TemporalAppendEntry::Observation { row } => {
                    let stream_key = resolve_stream(catalog, entity_key, &row.attribute_name, row.dataset_id.as_ref())?;
                    let metadata = row.metadata.as_ref();
                    let row = observation_row(stream_key, row, recorded_at, identity, HistoryMembers::default())?;
                    self.history.push((row, metadata));
                }
                TemporalAppendEntry::Tombstone {
                    attribute_name,
                    dataset_id,
                    attribute_type,
                    instance,
                } => {
                    let stream_key = resolve_stream(catalog, entity_key, attribute_name, dataset_id.as_ref())?;
                    self.history.push((
                        AttributeHistoryRow {
                            recorded_at,
                            identity,
                            stream_key,
                            // The family the ended instance belonged to is carried on
                            // the tombstone, which is what lets the render synthesise
                            // the null member that family prescribes.
                            attribute_kind: stored_attribute_kind(*attribute_type),
                            // A deletion records its instant and no typed value; the
                            // null representation is synthesised from the attribute
                            // family at render time (clauses 4.5.7 and 4.5.8).
                            value: AttributeHistoryValue::Tombstone { deleted_at: recorded_at },
                            axes: SemanticTimeAxes::default(),
                            instance: instance.clone(), // clone: the persisted row owns its stable instance identity.
                            members: HistoryMembers::default(),
                        },
                        None,
                    ));
                }
                TemporalAppendEntry::Scope {
                    observation,
                    observed_at,
                    instance,
                } => self.scopes.push(ScopeHistoryEntry {
                    recorded_at,
                    entity_id: intent.entity_id.clone(), // clone: each persisted scope row owns its entity identity.
                    identity,
                    value: match observation {
                        // clone: each persisted scope row owns its validated path.
                        ScopeObservation::Path(path) => ScopeHistoryValue::Path(path.clone()),
                        // Clause 4.18 reserves `urn:ngsi-ld:null` for a deleted
                        // scope, which the row records as a deletion instant with
                        // no path rather than as a sentinel path.
                        ScopeObservation::Deleted => ScopeHistoryValue::Tombstone { deleted_at: recorded_at },
                    },
                    // Clause 4.5.6: the temporal scope carries `observedAt`,
                    // `createdAt` and `modifiedAt`; a scope appended by this
                    // write is created and last modified at the write.
                    axes: SemanticTimeAxes {
                        observed_at: *observed_at,
                        created_at: Some(recorded_at),
                        modified_at: Some(recorded_at),
                    },
                    instance: instance.clone(), // clone: each persisted scope row owns its instance identity.
                }),
            }
        }
        Ok(())
    }
}

/// The catalogue rows one intent needs before its history rows can be stored.
///
/// Entity types come from the intent, which carries what the write said the
/// entity is (clause 4.5.6: the Temporal Evolution is "the Entity [...] as
/// mandated by clause 4.5.1"). They are not re-derived from the observation
/// payload: that payload is the request's Entity Fragment, whose `type` clauses
/// 5.6.2.4, 5.6.3.4 and 5.6.17.4 leave optional, and a bare Attribute Fragment
/// (clauses 5.6.4 and 5.6.19) carries an attribute family there rather than an
/// entity type at all. The intent's purpose travels beside the types, because
/// an evolution that is not there stops an observation and does not stop a
/// deletion.
fn catalog_upsert(intent: &TemporalAppendIntent) -> TemporalCatalogUpsert {
    let mut streams: Vec<TemporalStreamKey> = Vec::with_capacity(intent.entries.len());
    for entry in &intent.entries {
        let key = match entry {
            TemporalAppendEntry::Observation { row } => TemporalStreamKey {
                attribute_name: row.attribute_name.clone(), // clone: the catalogue grain owns the attribute identity.
                dataset_id: row.dataset_id.clone(),         // clone: the catalogue grain owns the dataset identity.
            },
            TemporalAppendEntry::Tombstone {
                attribute_name, dataset_id, ..
            } => TemporalStreamKey {
                attribute_name: attribute_name.clone(), // clone: the catalogue grain owns the attribute identity.
                dataset_id: dataset_id.clone(),         // clone: the catalogue grain owns the dataset identity.
            },
            TemporalAppendEntry::Scope { .. } => continue,
        };
        streams.push(key);
    }
    TemporalCatalogUpsert {
        entity_id: intent.entity_id.clone(), // clone: the catalogue upsert owns the entity identity.
        types: intent.entity_types.clone(),  // clone: the catalogue upsert owns the entity types it may create the evolution with.
        purpose: intent.write_purpose(),
        streams,
    }
}

/// The narrow key one intent's entries are recorded against, if it has any to
/// record.
///
/// Clause 5.6.16 deletes a Temporal Evolution on its own and clause 5.6.6
/// deletes current state on its own, so a deletion can reach an entity that no
/// longer has an evolution. Clauses 4.5.7 and 4.5.8 record a deletion as one
/// more instance of a sequence, and an entity with no evolution has no sequence
/// to add one to, so the intent records nothing and the chunk carries on. An
/// observation is different: the catalogue resolves or creates a key for every
/// one of them, so a missing key there is a catalogue invariant violation
/// rather than a routine outcome.
fn recording_entity_key(catalog: &TemporalCatalog, intent: &TemporalAppendIntent) -> Result<Option<TemporalEntityKey>, TemporalStoreError> {
    match catalog.entity_keys.get(&intent.entity_id).copied() {
        Some(entity_key) => Ok(Some(entity_key)),
        None => match intent.write_purpose() {
            TemporalWritePurpose::Deletion => Ok(None),
            // clone: the invariant owns the entity whose key the catalogue failed to resolve.
            TemporalWritePurpose::Observation => Err(TemporalStoreInvariant::EntityKeyUnresolved(intent.entity_id.clone()).into()),
        },
    }
}

/// Find the stream the chunk's catalogue assigned to one entity's attribute
/// grain.
///
/// A grain resolves against the entity it belongs to: the chunk catalogue keys
/// each stream by the narrow entity key beside the grain, so a history row needs
/// its entity's key before it can name its stream. The caller has already
/// resolved that key, so an entity whose evolution the chunk passed over never
/// reaches here.
fn resolve_stream(
    catalog: &TemporalCatalog,
    entity_key: TemporalEntityKey,
    attribute_name: &AttributeName,
    dataset_id: Option<&Identifier>,
) -> Result<StreamKey, TemporalStoreError> {
    let key = TemporalStreamKey {
        attribute_name: attribute_name.clone(), // clone: the lookup key follows the catalogue's owned natural key.
        dataset_id: dataset_id.cloned(),
    };
    catalog
        .streams
        .get(&(entity_key, key))
        .copied()
        // clone: the invariant owns the attribute whose stream the catalogue failed to return.
        .ok_or_else(|| TemporalStoreInvariant::StreamUnresolved(attribute_name.clone()).into())
}

/// Build one history row from a neutral observation and the `members` its
/// metadata stores as.
///
/// # Errors
///
/// Returns [`TemporalStoreError`] when the observation's value does not read
/// as a stored row.
pub(crate) fn observation_row(
    stream_key: StreamKey,
    row: &TemporalHistoryRow,
    recorded_at: DateTime<Utc>,
    identity: TemporalObservationIdentity,
    members: HistoryMembers,
) -> Result<AttributeHistoryRow, TemporalStoreError> {
    Ok(AttributeHistoryRow {
        recorded_at,
        identity,
        stream_key,
        attribute_kind: stored_attribute_kind(row.attribute_type),
        value: AttributeHistoryValue::of_observation(&row.value)?,
        axes: SemanticTimeAxes {
            // Each axis is preserved exactly as the observation carried it. An
            // observation that declared none keeps none, so it matches no window
            // on that Temporal Property (clauses 4.11 and 5.2.21).
            observed_at: row.observed_at,
            created_at: row.created_at,
            modified_at: row.modified_at,
        },
        instance: row.instance.clone(), // clone: the persisted row owns its instance identity.
        members,
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_history_value::AttributeHistoryValue,
        error::TemporalStoreError,
        history_members::HistoryMembers,
        intent_sink::{IntentRows, recording_entity_key},
        temporal_catalog_write::TemporalCatalog,
        temporal_store_invariant::TemporalStoreInvariant,
        temporal_stream::TemporalStreamKey,
    };
    use andromeda_context_commit::{write_id::WriteId, write_ordinal::WriteOrdinal, write_record::WriteRecord};
    use andromeda_ngsi_ld_entity::geometry_value::GeometryValue;
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use andromeda_ngsi_ld_lexicon::{
        attribute_name::AttributeName,
        attribute_type::AttributeType,
        identifier::Identifier,
        instance_identity::InstanceIdentity,
    };
    use andromeda_ngsi_ld_model::temporal::{TemporalHistoryRow, TemporalValue};
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_postgresql_codes::{stream_key::StreamKey, temporal_entity_key::TemporalEntityKey};
    use andromeda_temporal::append_intent::{TemporalAppendEntry, TemporalAppendIntent};
    use chrono::{TimeZone, Utc};
    use serde_json::json;
    use std::collections::HashMap;

    const SPEED: &str = "https://example.com/speed";

    /// One intent over `urn:ngsi-ld:Vehicle:1` carrying exactly `entries`.
    fn intent(entries: Vec<TemporalAppendEntry>) -> TemporalAppendIntent {
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        TemporalAppendIntent {
            record: WriteRecord::restored(WriteId::mint(), recorded_at),
            base_ordinal: WriteOrdinal::at(0),
            entity_id: "urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("valid entity URI"),
            entity_types: Vec::new(),
            entries,
        }
    }

    /// The end of the default instance of `speed`.
    fn tombstone() -> TemporalAppendEntry {
        TemporalAppendEntry::Tombstone {
            attribute_name: SPEED.parse::<AttributeName>().expect("valid attribute name"),
            dataset_id: None,
            attribute_type: AttributeType::Property,
            instance: InstanceIdentity::generate(),
        }
    }

    /// One observation of `speed`.
    fn observation() -> TemporalAppendEntry {
        TemporalAppendEntry::Observation {
            row: observed_row(AttributeType::Property, TemporalValue::Number(JsonNumber::from(42))),
        }
    }

    /// One observation of `speed`, of family `attribute_type`, holding `value`.
    fn observed_row(attribute_type: AttributeType, value: TemporalValue) -> TemporalHistoryRow {
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        TemporalHistoryRow {
            attribute_name: SPEED.parse::<AttributeName>().expect("valid attribute name"),
            attribute_type,
            dataset_id: None,
            value,
            observed_at: None,
            recorded_at,
            created_at: Some(recorded_at),
            modified_at: Some(recorded_at),
            instance: InstanceIdentity::generate(),
            metadata: None,
            partition_total: None,
        }
    }

    /// A catalogue that resolved nothing, as one whose entities all had their
    /// Temporal Evolution removed by clause 5.6.16 would.
    fn empty_catalog() -> TemporalCatalog {
        TemporalCatalog {
            entity_keys: HashMap::new(),
            streams: HashMap::new(),
        }
    }

    /// Clause 5.6.16 removes a Temporal Evolution on its own while clause 5.6.6
    /// removes current state on its own, so a deletion can meet an entity that
    /// has no evolution left. Clauses 4.5.7 and 4.5.8 record a deletion as one
    /// more instance of a sequence, and there is no sequence: the intent is
    /// passed over instead of failing the chunk.
    #[test]
    fn a_deletion_whose_evolution_is_gone_records_nothing_rather_than_failing() {
        let resolved = recording_entity_key(&empty_catalog(), &intent(vec![tombstone()])).expect("a deletion tolerates a missing evolution");
        assert!(resolved.is_none());
    }

    /// The catalogue resolves or creates a key for every observation, so an
    /// observation that reaches the sink without one is a catalogue invariant
    /// violation and has to be reported rather than silently dropped.
    #[test]
    fn an_observation_whose_evolution_is_gone_is_reported_rather_than_dropped() {
        let error = recording_entity_key(&empty_catalog(), &intent(vec![observation()])).expect_err("an observation needs its evolution");
        assert!(
            matches!(error, TemporalStoreError::Invariant(TemporalStoreInvariant::EntityKeyUnresolved(_))),
            "{error:?}"
        );
    }

    /// A resolved key is handed back whatever the intent is for.
    #[test]
    fn a_resolved_key_is_returned_for_a_deletion_and_for_an_observation() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("valid entity URI");
        let entity_key = TemporalEntityKey::try_from(7_i64).expect("a positive narrow key");
        let catalog = TemporalCatalog {
            entity_keys: HashMap::from([(entity_id, entity_key)]),
            streams: HashMap::new(),
        };
        for entries in [vec![tombstone()], vec![observation()]] {
            let resolved = recording_entity_key(&catalog, &intent(entries)).expect("a resolved key is not an error");
            pretty_assertions::assert_eq!(resolved, Some(entity_key));
        }
    }

    /// Clauses 4.5.7 and 4.5.8: a deletion records its instant and no value,
    /// so no member a value carries beside it is stored either.
    #[test]
    fn a_tombstone_stores_no_metadata() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("valid entity URI");
        let entity_key = TemporalEntityKey::try_from(7_i64).expect("a positive narrow key");
        let grain = TemporalStreamKey {
            attribute_name: SPEED.parse::<AttributeName>().expect("valid attribute name"),
            dataset_id: None,
        };
        let catalog = TemporalCatalog {
            entity_keys: HashMap::from([(entity_id, entity_key)]),
            streams: HashMap::from([((entity_key, grain), StreamKey::try_from(9_i64).expect("a positive stream key"))]),
        };
        let intent = intent(vec![tombstone()]);
        let mut rows = IntentRows::default();
        rows.extend(&catalog, &intent).expect("the tombstone records");
        let [(row, metadata)] = rows.history.as_slice() else {
            panic!("one row: {:?}", rows.history);
        };
        assert!(metadata.is_none());
        pretty_assertions::assert_eq!(row.members, HistoryMembers::default());
        assert!(matches!(row.value, AttributeHistoryValue::Tombstone { .. }));
    }

    /// The observation carries the geometry already built, so the row stores
    /// its positions as they stand rather than a JSON rendering of them, and a
    /// geometry without members leaves the members column empty.
    #[test]
    fn a_geometry_observation_is_stored_as_its_geometry() {
        let (form, _) =
            serde_json::from_value::<Geometry>(json!({"type": "LineString", "coordinates": [[0.0, 0.0, 10.0], [1.0, 2.0, 30.0], [3.0, 4.0, 20.0]]}))
                .expect("a valid LineString")
                .into_parts();
        let row = observed_row(AttributeType::GeoProperty, TemporalValue::Geometry(GeometryValue::new(form.clone()))); // clone: the test compares the stored value against what it observed
        let AttributeHistoryValue::Geometry { form: stored, members } = AttributeHistoryValue::of_observation(&row.value).expect("a geometry is storable")
        else {
            panic!("a geometry is stored in the geometry column");
        };
        pretty_assertions::assert_eq!(stored, form);
        assert!(members.is_none());
    }
}
