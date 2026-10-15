//! Preparing the durable intent the history consequences of one write record.

use crate::{
    append_intent::{TemporalAppendEntry, TemporalAppendIntent},
    error::TemporalError,
    observed_instance::ObservedInstance,
    reified_scope::scope_observations,
    scope_observation::ScopeObservation,
};
use andromeda_context_commit::{history_record::HistoryRecord, submission_refusal::SubmissionRefusal, write_ordinal::WriteOrdinal, write_record::WriteRecord};
use andromeda_ngsi_ld_entity::{
    document::{EntityDocument, EntityDocumentParts, attribute_member::AttributeMember, instance_document::InstanceDocument, slot::Slot},
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    entity_type_name::EntityTypeName,
    identifier::Identifier,
    instance_family::InstanceFamily,
    instance_identity::InstanceIdentity,
    ngsi_ld_date_time::NgsiLdDateTime,
};
use andromeda_ngsi_ld_model::temporal::{StreamExtent, TemporalHistoryRow};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use chrono::{DateTime, Utc};

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
                // the one form every observation is read in.
                append_document_entries(EntityDocument::from(state.as_ref()), recorded_at, &mut entries)?;
            }
            HistoryRecord::EvolutionInstances(document) => {
                // clone: every history row owns the values it records, while the
                // written document stays shared with the write it belongs to.
                append_document_entries(EntityDocument::clone(document), recorded_at, &mut entries)?;
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
            HistoryRecord::ScopeRemoval => entries.push(TemporalAppendEntry::Scope {
                observation: ScopeObservation::Deleted,
                observed_at: Some(recorded_at),
                instance: InstanceIdentity::generate(),
            }),
        }
    }
    Ok(Some(TemporalAppendIntent {
        record,
        base_ordinal,
        entity_id: entity_id.clone(), // clone: the durable intent owns the validated identifier.
        entity_types: entity_types.to_vec(),
        entries,
    }))
}

/// Append the attribute and scope entries of one observed document.
fn append_document_entries(document: EntityDocument, recorded_at: DateTime<Utc>, entries: &mut Vec<TemporalAppendEntry>) -> Result<(), TemporalError> {
    let EntityDocumentParts { scope, attributes, .. } = document.into_parts();
    match scope {
        Some(Slot::Set(scopes)) => entries.extend(scopes.as_slice().iter().map(|path| {
            scope_entry(ScopeObservation::Path(path.clone()), recorded_at) // clone: the durable intent owns its paths.
        })),
        Some(Slot::Remove) => entries.push(scope_entry(ScopeObservation::Deleted, recorded_at)),
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
            if let Some(observed) = ObservedInstance::read(instance)? {
                entries.push(observation_entry(&attribute_name, observed, recorded_at));
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
            instance: instance_id.map_or_else(InstanceIdentity::generate, |provided| {
                InstanceIdentity::from_identifier(provided, InstanceFamily::Attribute)
            }),
            metadata,
            stream_extent: StreamExtent::Whole,
        },
    }
}

/// One scope entry, minted its own stable identity.
fn scope_entry(observation: ScopeObservation, observed_at: DateTime<Utc>) -> TemporalAppendEntry {
    TemporalAppendEntry::Scope {
        observation,
        observed_at: Some(observed_at),
        instance: InstanceIdentity::generate(),
    }
}

/// Append the scope entries one reified scope instance records.
///
/// Clause 4.5.6 represents the Scope of a Temporal Evolution as "a temporal
/// representation of a Property", whose `value` holds the paths and whose
/// `observedAt` dates them; one the client did not date is dated at the write.
/// What the `value` states is read by `crate::reified_scope`, which the write
/// already held the instance to.
fn append_reified_scope(instance: &InstanceDocument, recorded_at: DateTime<Utc>, entries: &mut Vec<TemporalAppendEntry>) -> Result<(), TemporalError> {
    let observed_at = match instance.observed_at() {
        Some(Slot::Set(text)) => text
            .as_str()
            .parse::<NgsiLdDateTime>()
            .map(NgsiLdDateTime::get)
            .map_err(|source| TemporalError::from(SubmissionRefusal::ObservedAt(source)))?,
        Some(Slot::Remove) | None => recorded_at,
    };
    let observations = scope_observations(instance.value()).map_err(|refusal| TemporalError::from(EntityRefusal::Scope(refusal)))?;
    entries.extend(observations.into_iter().map(|observation| scope_entry(observation, observed_at)));
    Ok(())
}

#[cfg(test)]
mod tests;
