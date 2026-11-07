//! Preparing the durable intent the history consequences of one write record.

use crate::{
    append_intent::{TemporalAppendEntry, TemporalAppendIntent},
    error::TemporalError,
    observed_instance::{ObservedInstance, declared_observed_at},
    reified_scope::scope_observations,
    scope_entry_kind::ScopeEntryKind,
    scope_observation::ScopeObservation,
    written_instance::{DeletedInstance, WrittenInstance},
};
use andromeda_context_commit::{
    history_record::HistoryRecord,
    submission_refusal::{RepeatedInstanceId, SubmissionRefusal},
    write_ordinal::WriteOrdinal,
    write_record::WriteRecord,
};
use andromeda_ngsi_ld_entity::{
    document::{EntityDocument, EntityDocumentParts, attribute_member::AttributeMember, instance_document::InstanceDocument, slot::Slot},
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier, instance_identity::InstanceIdentity};
use andromeda_ngsi_ld_model::temporal::{StreamExtent, TemporalHistoryRow, TemporalValue, evolution_instance::EvolutionInstance};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use chrono::{DateTime, Utc};
use std::collections::HashSet;

/// Prepare one typed intent from the history consequences of an atomic write.
///
/// `record` and `base_ordinal` are the commit's, not minted here: every entity a
/// commit lands shares the commit's single write identity, and this intent's
/// entries are numbered starting at `base_ordinal` in the commit's one running
/// ordinal counter rather than each entity restarting its own count at zero.
///
/// `entity_types` is likewise the write's, not the history's: clause 4.5.6
/// makes the Temporal Evolution "the Entity [...] as mandated by clause 4.5.1",
/// and the observed payload of an ordinary attribute write is an Entity
/// Fragment whose `type` clauses 5.6.2.4, 5.6.3.4 and 5.6.17.4 leave optional.
pub(crate) fn prepare(
    entity_id: &Identifier,
    entity_types: &[EntityTypeName],
    history: &[HistoryRecord],
    record: WriteRecord,
    base_ordinal: WriteOrdinal,
) -> Result<Option<TemporalAppendIntent>, TemporalError> {
    if history.is_empty() {
        return Ok(None);
    }
    let recorded_at = record.recorded_at();
    let mut entries = Vec::new();
    for consequence in history {
        match consequence {
            HistoryRecord::Observations(state) => {
                // History rows own what they record, so the observed state is
                // read as the written document whose instances they take over,
                // the one form every observation is read in. A Core write's
                // observation carries a Scope only when the write changed it.
                append_document_entries(EntityDocument::from(state.as_ref()), recorded_at, ScopeEntryKind::Event, &mut entries)?;
            }
            HistoryRecord::EvolutionInstances(document) => {
                // clone: every history row owns the values it records, while the
                // written document stays shared with the write it belongs to.
                append_document_entries(EntityDocument::clone(document), recorded_at, ScopeEntryKind::Assertion, &mut entries)?;
            }
            HistoryRecord::Tombstone(removed) => entries.push(TemporalAppendEntry::Tombstone {
                attribute_name: removed.key.attr.clone(),   // clone: the durable intent owns the instance identity.
                dataset_id: removed.key.dataset_id.clone(), // clone: the durable intent owns the dataset identity.
                attribute_type: removed.attribute_type,
                instance: InstanceIdentity::generate(),
            }),
            // Clause 4.18 ends a Scope's validity when it is "explicitly
            // removed by an update or delete operation", and the write that
            // removed it declared no instant of its own, so the removal is
            // dated at the write — which is also what clause 4.5.6 asks of a
            // Scope changed through the Core API, "the observedAt sub-Property
            // should be set as a copy of the modifiedAt sub-Property".
            HistoryRecord::ScopeRemoval => entries.push(scope_entry(ScopeObservation::Deleted, ScopeEntryKind::Event, recorded_at)),
        }
    }
    refuse_repeated_instance_ids(&entries)?;
    Ok(Some(TemporalAppendIntent {
        record,
        base_ordinal,
        entity_id: entity_id.clone(), // clone: the durable intent owns the validated identifier.
        entity_types: entity_types.to_vec(),
        entries,
    }))
}

/// Append the attribute and scope entries of one observed document, its
/// non-reified `scope` being of `stated` kind.
///
/// The non-reified `scope` carries no instant of its own and is dated at the
/// write. Its NGSI-LD Null removes the Scope (clause 4.18), which is an event
/// whatever the Scope stated; a reified scope instance is an event dated by
/// its own `observedAt`.
fn append_document_entries(
    document: EntityDocument,
    recorded_at: DateTime<Utc>,
    stated: ScopeEntryKind,
    entries: &mut Vec<TemporalAppendEntry>,
) -> Result<(), TemporalError> {
    let EntityDocumentParts { scope, attributes, .. } = document.into_parts();
    match scope {
        Some(Slot::Set(scopes)) => entries.extend(scopes.as_slice().iter().map(|path| {
            scope_entry(ScopeObservation::Path(path.clone()), stated, recorded_at) // clone: the durable intent owns its paths.
        })),
        Some(Slot::Remove) => entries.push(scope_entry(ScopeObservation::Deleted, ScopeEntryKind::Event, recorded_at)),
        None => {}
    }
    for (attribute_name, member) in attributes {
        let AttributeMember::Instances(instances) = member else {
            continue;
        };
        if attribute_name.as_str() == expanded_properties::SCOPE.as_str() {
            for instance in &instances {
                append_reified_scope(instance, recorded_at, entries)?;
            }
            continue;
        }
        for instance in instances {
            match WrittenInstance::read(instance)? {
                Some(WrittenInstance::Observation(observed)) => entries.push(observation_entry(&attribute_name, observed, recorded_at)),
                Some(WrittenInstance::Deletion(deleted)) => entries.push(deletion_entry(&attribute_name, deleted, recorded_at)),
                None => {}
            }
        }
    }
    Ok(())
}

/// The entry one observed attribute instance records.
fn observation_entry(attribute_name: &AttributeName, observed: ObservedInstance, recorded_at: DateTime<Utc>) -> TemporalAppendEntry {
    let ObservedInstance {
        attribute_type,
        dataset_id,
        value,
        observed_at,
        instance_id,
        metadata,
    } = observed;
    TemporalAppendEntry::Observation {
        row: TemporalHistoryRow {
            attribute_name: attribute_name.clone(), // clone: each observation owns its attribute name.
            attribute_type,
            dataset_id,
            value,
            // An `observedAt` the client did not declare stays absent: clause
            // 5.2.21 makes it a Temporal Property of the observation, so
            // substituting the write clock would invent one.
            observed_at,
            recorded_at,
            // Clause 4.8 has the broker assign `createdAt` and `modifiedAt`
            // itself, and a newly appended instance is created and last
            // modified at the instant the write was accepted.
            created_at: Some(recorded_at),
            modified_at: Some(recorded_at),
            instance: EvolutionInstance::Attribute(instance_id.map_or_else(InstanceIdentity::generate, InstanceIdentity::from_identifier)),
            metadata,
            stream_extent: StreamExtent::Whole,
        },
    }
}

/// The entry one deletion a written Temporal Evolution states records.
///
/// The deletion is an instance the write adds to the evolution, so it is
/// entered as one and the write that states it brings the evolution into
/// being like any other (clause 5.6.11.4). Clause 4.8 has the broker generate
/// `deletedAt`, so the deletion is recorded at the instant the write was
/// accepted, and the `observedAt` the client declared places it on that axis.
/// Like the end of an instance a Core write deleted, it carries no `createdAt`
/// or `modifiedAt` and no metadata: clauses 4.5.7 and 4.5.8 render it as the
/// Null member of its family beside `deletedAt`.
fn deletion_entry(attribute_name: &AttributeName, deleted: DeletedInstance, recorded_at: DateTime<Utc>) -> TemporalAppendEntry {
    let DeletedInstance {
        attribute_type,
        dataset_id,
        observed_at,
        instance_id,
    } = deleted;
    TemporalAppendEntry::Observation {
        row: TemporalHistoryRow {
            attribute_name: attribute_name.clone(), // clone: each entry owns its attribute name.
            attribute_type,
            dataset_id,
            value: TemporalValue::Tombstone { deleted_at: recorded_at },
            observed_at,
            recorded_at,
            created_at: None,
            modified_at: None,
            instance: EvolutionInstance::Attribute(instance_id.map_or_else(InstanceIdentity::generate, InstanceIdentity::from_identifier)),
            metadata: None,
            stream_extent: StreamExtent::Whole,
        },
    }
}

/// Refuse entries giving one `instanceId` to two instances of one attribute.
///
/// Clause 4.5.7 makes an `instanceId` name one instance of an attribute, across
/// its `datasetId`s. Only a Temporal API write keeps the identifier a client
/// gave (a Core API write ignores it, clause 5.2.5), so only its entries can
/// repeat one. Refusing the repeat here names the write it came from, which
/// the instance claims the history store stakes cannot: they meet it inside a
/// statement every write of the commit shares.
///
/// # Errors
///
/// Returns [`SubmissionRefusal::RepeatedInstanceId`] for the first identifier
/// an attribute's instances repeat.
fn refuse_repeated_instance_ids(entries: &[TemporalAppendEntry]) -> Result<(), TemporalError> {
    let mut seen = HashSet::new();
    let repeated = entries
        .iter()
        .filter_map(|entry| match entry {
            TemporalAppendEntry::Observation { row } => match &row.instance {
                EvolutionInstance::Attribute(instance) => Some((&row.attribute_name, instance)),
                EvolutionInstance::Scope => None,
            },
            TemporalAppendEntry::Tombstone { .. } | TemporalAppendEntry::Scope { .. } => None,
        })
        .find(|named| !seen.insert(*named));
    match repeated {
        Some((attribute, instance)) => Err(SubmissionRefusal::RepeatedInstanceId(Box::new(RepeatedInstanceId {
            attribute: attribute.clone(), // clone: the refusal outlives the entries it names.
            instance: instance.clone(),   // clone: the refusal outlives the entries it names.
        }))
        .into()),
        None => Ok(()),
    }
}

/// One scope entry.
const fn scope_entry(observation: ScopeObservation, kind: ScopeEntryKind, observed_at: DateTime<Utc>) -> TemporalAppendEntry {
    TemporalAppendEntry::Scope {
        observation,
        kind,
        observed_at: Some(observed_at),
    }
}

/// Append the scope entries one reified scope instance records.
///
/// Clause 4.5.6 represents the Scope of a Temporal Evolution as "a temporal
/// representation of a Property", whose `value` holds the paths and whose
/// `observedAt` dates them; one the client did not date is dated at the write.
/// An instance carrying a member beyond the four Temporal Properties clause
/// 4.5.6 gives a reified Scope is refused, as it is on the client's write.
fn append_reified_scope(instance: &InstanceDocument, recorded_at: DateTime<Utc>, entries: &mut Vec<TemporalAppendEntry>) -> Result<(), TemporalError> {
    let observed_at = declared_observed_at(instance.observed_at()).unwrap_or(recorded_at);
    let observations = scope_observations(instance).map_err(|refusal| TemporalError::from(EntityRefusal::Scope(refusal)))?;
    entries.extend(
        observations
            .into_iter()
            .map(|observation| scope_entry(observation, ScopeEntryKind::Event, observed_at)),
    );
    Ok(())
}

#[cfg(test)]
mod tests;
