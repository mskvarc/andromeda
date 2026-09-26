//! Projecting one unapplied temporal intent into the observations a query sees.
//!
//! A deferred sink accepts a write as an intent and applies it later, so a read
//! taken in between has to add what the intent will land. This module owns that
//! projection: which of an intent's entries the compiled query selects, and
//! what observation each of them becomes. [`crate::pending_overlay`] owns the
//! other half — merging those observations with the stored ones and cutting the
//! result the way the store would have.
//!
//! The Scope is the one entry that does not project one-for-one. Clause 4.18
//! gives an Entity at most one instance of the scope Property and renders more
//! than one Scope as a JSON array, while clause 4.5.7 lets one write state
//! several instances "each one representing an instance of the referred
//! Property [...] at a particular point in time", so the entries are folded by
//! the instant they declare.

use crate::{
    append_intent::{SequencedTemporalAppendIntent, TemporalAppendEntry, TemporalObservationIdentity},
    error::TemporalError,
    port::history_reader::TemporalHistoryObservation,
    scope_observation::ScopeObservation,
};
use andromeda_entity::{query_residual::ResidualFilter, residual_filter};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, instance_family::InstanceFamily, time_prop::TimeProp};
use andromeda_ngsi_ld_model::{
    scope_instance_value::scope_instance_value,
    temporal::{TemporalHistoryRow, TemporalValue},
};
use andromeda_ngsi_ld_query::{
    dataset::DatasetFilter,
    temporal::{compiled::CompiledTemporalQuery, window::TimeInterval},
};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use chrono::{DateTime, Utc};

/// Return whether an intent contributes both entity selection and history rows.
///
/// # Errors
///
/// Returns [`TemporalError::Internal`] when the intent's selection value
/// carries an `@id` or `@type` the selection cannot read: the write that
/// queued the intent admitted both.
pub(crate) fn intent_matches_query(
    sequenced: &SequencedTemporalAppendIntent,
    compiled: &CompiledTemporalQuery,
    selection: &ResidualFilter,
) -> Result<bool, TemporalError> {
    let selection_matches = match sequenced.intent.selection_value.as_ref() {
        Some(value) => residual_filter::matches(selection, value).map_err(TemporalError::internal)?,
        None => false,
    };
    Ok(selection_matches && !pending_observations(sequenced, compiled).is_empty())
}

/// Project matching history entries from one pending intent.
///
/// The Scope entries are folded out of the run first. Clause 4.18 gives an
/// Entity at most one instance of the scope Property, so however many Scopes one
/// write named it recorded one instance, and the intent carries one entry per
/// path only because storage does.
pub(crate) fn pending_observations(sequenced: &SequencedTemporalAppendIntent, compiled: &CompiledTemporalQuery) -> Vec<TemporalHistoryObservation> {
    let recorded_at = sequenced.intent.record.recorded_at();
    let mut observations: Vec<TemporalHistoryObservation> = Vec::new();
    let mut event: Vec<(TemporalObservationIdentity, &TemporalAppendEntry)> = Vec::new();
    for (identity, entry) in sequenced.intent.history_entries() {
        match entry {
            TemporalAppendEntry::Scope { observed_at, .. } => {
                // One write states several instances when it dates them apart
                // (clause 4.5.7), and one instance holding an array when it
                // does not (clause 4.18), so the declared instant closes the
                // run rather than the write does.
                if event.first().is_some_and(|(_, first)| scope_instant(first) != *observed_at) {
                    observations.extend(fold_pending_scope(&event, recorded_at));
                    event.clear();
                }
                event.push((identity, entry));
            }
            TemporalAppendEntry::Observation { .. } | TemporalAppendEntry::Tombstone { .. } => {
                if let Some(observation) = pending_observation(identity, entry, recorded_at) {
                    observations.push(observation);
                }
            }
        }
    }
    observations.extend(fold_pending_scope(&event, recorded_at));
    observations.retain(|observation| matches_history_row(&observation.row, compiled));
    observations
}

/// The instant a Scope entry declares, and nothing for any other entry.
const fn scope_instant(entry: &TemporalAppendEntry) -> Option<DateTime<Utc>> {
    match entry {
        TemporalAppendEntry::Scope { observed_at, .. } => *observed_at,
        TemporalAppendEntry::Observation { .. } | TemporalAppendEntry::Tombstone { .. } => None,
    }
}

/// Fold one intent's Scope entries that share an instant into the single
/// instance clause 4.18 allows an Entity, keyed by the scope IRI so it renders
/// down the same path as the stored one (clause 4.5.6).
///
/// The intent's first Scope entry supplies the published identity and the
/// Temporal Properties, exactly as the first stored row of the write does. A
/// removal never shares a write with a path — clause 4.18 admits the NGSI-LD
/// Null "only in case of deleted scopes" — so a run whose first entry is a
/// removal is a removal.
fn fold_pending_scope(entries: &[(TemporalObservationIdentity, &TemporalAppendEntry)], recorded_at: DateTime<Utc>) -> Option<TemporalHistoryObservation> {
    let (identity, first) = entries.first()?;
    let TemporalAppendEntry::Scope { observed_at, instance, .. } = first else {
        return None;
    };
    let mut paths = Vec::with_capacity(entries.len());
    let mut removed = false;
    for (_, entry) in entries {
        if let TemporalAppendEntry::Scope { observation, .. } = entry {
            match observation {
                ScopeObservation::Deleted => removed = true,
                ScopeObservation::Path(path) => paths.push(path.clone()), // clone: the overlay row owns the paths the intent holds.
            }
        }
    }
    let value = if removed {
        TemporalValue::Tombstone { deleted_at: recorded_at }
    } else {
        scope_instance_value(&paths)?
    };
    let attribute_name = expanded_properties::SCOPE.as_str().parse::<AttributeName>().ok()?;
    Some(TemporalHistoryObservation {
        row: TemporalHistoryRow {
            attribute_name,
            // Clause 4.5.6 makes the reified Scope "a temporal representation
            // of a Property (clause 4.5.7)", which annex C clause C.5.16
            // renders as `"type": "Property"`.
            attribute_type: AttributeType::Property,
            // Clause 4.5.4 gives a Scope no dataset dimension.
            dataset_id: None,
            value,
            observed_at: *observed_at,
            recorded_at,
            // Clause 4.5.6: a Scope appended by this write is created and last
            // modified at the write, the same two instants the sink stores.
            created_at: Some(recorded_at),
            modified_at: Some(recorded_at),
            instance: instance.clone(), // clone: the overlay row owns its published identity.
            metadata: None,
            partition_total: None,
        },
        identity: Some(*identity),
    })
}

/// Convert one pending attribute history entry into a provenance-bearing
/// observation.
fn pending_observation(identity: TemporalObservationIdentity, entry: &TemporalAppendEntry, recorded_at: DateTime<Utc>) -> Option<TemporalHistoryObservation> {
    let row = match entry {
        TemporalAppendEntry::Observation { row } => row.clone(), // clone: the overlay result owns a neutral row while the durable intent remains reusable.
        TemporalAppendEntry::Tombstone {
            attribute_name,
            dataset_id,
            attribute_type,
            instance,
        } => TemporalHistoryRow {
            attribute_name: attribute_name.clone(), // clone: the overlay row owns its attribute identity.
            // The family the ended instance belonged to, so a pending deletion
            // renders the same null member the stored one will (clauses 4.5.7
            // and 4.5.8).
            attribute_type: *attribute_type,
            dataset_id: dataset_id.clone(), // clone: the overlay row owns its dataset identity.
            // A deletion is positioned on the `deletedAt` axis alone; it
            // declares no observation instant and modifies nothing.
            value: TemporalValue::Tombstone { deleted_at: recorded_at },
            observed_at: None,
            recorded_at,
            created_at: None,
            modified_at: None,
            instance: instance.clone(), // clone: the overlay row owns its instance identity.
            metadata: None,
            partition_total: None,
        },
        // The Scope is folded by `fold_pending_scope` instead: one write is one
        // instance (clause 4.18), so it cannot be projected one entry at a time.
        TemporalAppendEntry::Scope { .. } => return None,
    };
    Some(TemporalHistoryObservation { row, identity: Some(identity) })
}

/// Whether a row is the reified Scope rather than an Attribute instance.
///
/// Clause 4.18 gives the Scope an attribute-shaped temporal representation
/// keyed by the scope IRI, so the IRI is what separates the two once a row has
/// been built (clause 4.5.6).
pub(crate) fn is_scope_row(row: &TemporalHistoryRow) -> bool {
    InstanceFamily::of_attribute_name(&row.attribute_name) == InstanceFamily::Scope
}

/// Apply query instance and temporal constraints to one pending row.
fn matches_history_row(row: &TemporalHistoryRow, compiled: &CompiledTemporalQuery) -> bool {
    if !compiled.instances.attr_filter.is_empty()
        && !compiled
            .instances
            .attr_filter
            .iter()
            .any(|attribute| attribute.as_str() == row.attribute_name.as_str())
    {
        return false;
    }
    if compiled.window.timeproperty == TimeProp::Deleted && !matches!(row.value, TemporalValue::Tombstone { .. }) {
        return false;
    }
    if is_scope_row(row) {
        // Clause 4.5.4 gives a Scope no dataset dimension, so a `datasetId`
        // selection has nothing to say about it. Clause 4.18 makes it valid
        // from when it was set until it was removed, and a pending event is
        // the newest the Entity has, so its interval reaches the present and
        // overlaps every window that has not already closed before it.
        return window_closes_after(row.time_axis(compiled.window.timeproperty), compiled.window.interval);
    }
    if !matches_dataset(row, compiled) {
        return false;
    }
    matches_interval(row.time_axis(compiled.window.timeproperty), compiled.window.interval)
}

/// Whether the window is still open at `instant`, which is what a validity
/// interval running from `instant` to the present needs in order to overlap it.
const fn window_closes_after(instant: Option<DateTime<Utc>>, interval: TimeInterval) -> bool {
    let Some(instant) = instant else {
        return false;
    };
    match interval {
        TimeInterval::Unbounded | TimeInterval::After(_) => true,
        TimeInterval::Before(to) | TimeInterval::Between { to, .. } => instant.timestamp_micros() < to.timestamp_micros(),
    }
}

/// Apply the compiled dataset selector to one pending row.
fn matches_dataset(row: &TemporalHistoryRow, compiled: &CompiledTemporalQuery) -> bool {
    match DatasetFilter::from_selectors(&compiled.instances.dataset_id_filter) {
        DatasetFilter::Unfiltered => true,
        DatasetFilter::DefaultOnly => row.dataset_id.is_none(),
        DatasetFilter::NamedOnly(named) => row.dataset_id.as_ref().is_some_and(|dataset| named.contains(dataset)),
        DatasetFilter::DefaultOrNamed(named) => row.dataset_id.as_ref().is_none_or(|dataset| named.contains(dataset)),
    }
}

/// Return whether a temporal position falls inside the half-open query interval.
///
/// A row that carries no instant on the selected axis takes no position, so it
/// matches every bounded interval no better than a row outside it: clause 4.11
/// filters on the requested Temporal Property, and admitting an observation
/// that has none would answer the query with a match it cannot justify. An
/// unbounded query asks for no Temporal Property at all and keeps the row.
fn matches_interval(position: Option<DateTime<Utc>>, interval: TimeInterval) -> bool {
    match interval {
        TimeInterval::Unbounded => true,
        TimeInterval::Before(instant) => position.is_some_and(|position| position < instant),
        TimeInterval::After(instant) => position.is_some_and(|position| position >= instant),
        TimeInterval::Between { from, to } => position.is_some_and(|position| position >= from && position < to),
    }
}
