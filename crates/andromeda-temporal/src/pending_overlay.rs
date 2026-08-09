//! Immediate logical temporal reads over a deferred history sink.

use crate::{
    append_intent::{SequencedTemporalAppendIntent, TemporalAppendEntry, TemporalObservationIdentity},
    error::TemporalError,
    outcome::{TemporalEntityCandidate, TemporalEntityIdOutcome, TemporalPaginationOutcome, TemporalResidual},
    port::{
        history_reader::{
            AggregationPushdown,
            HistoryVisibility,
            TemporalHistoryObservation,
            TemporalHistoryOutcome,
            TemporalHistoryReader,
            TemporalReaderCapabilities,
        },
        pending_intent_reader::PendingTemporalIntentReader,
    },
};
use andromeda_entity::{query_residual::ResidualFilter, residual_filter};
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, time_prop::TimeProp, uri::Uri};
use andromeda_ngsi_ld_model::temporal::{TemporalAggregationRow, TemporalHistoryRow, TemporalValue};
use andromeda_ngsi_ld_query::{
    dataset::DatasetFilter,
    pagination::{CountPolicy, Pagination},
    temporal::{compiled::CompiledTemporalQuery, window::TimeInterval},
};
use async_trait::async_trait;
use std::{collections::HashSet, sync::Arc};

/// Reader that overlays committed pending intents on an idempotent history sink.
pub struct PendingTemporalHistoryOverlay {
    /// Selected history sink reader.
    stored: Arc<dyn TemporalHistoryReader>,
    /// Anchor-owned pending intent reader.
    pending: Arc<dyn PendingTemporalIntentReader>,
}

impl PendingTemporalHistoryOverlay {
    /// Construct an immediate logical view over an eventual append sink.
    pub const fn new(stored: Arc<dyn TemporalHistoryReader>, pending: Arc<dyn PendingTemporalIntentReader>) -> PendingTemporalHistoryOverlay {
        PendingTemporalHistoryOverlay { stored, pending }
    }

    /// Read all matching rows before the overlay applies lastN to the union.
    async fn complete_entity_history(&self, entity_id: &Uri, compiled: &CompiledTemporalQuery) -> Result<Vec<TemporalHistoryObservation>, TemporalError> {
        let mut unwindowed = compiled.clone();
        unwindowed.instances.last_n = None;
        // Snapshot anchor intents first. If delivery transitions after this read,
        // the subsequent sink read either sees the accepted rows or the retained
        // intent still supplies them. Reading in the opposite order creates a
        // gap when acceptance and acknowledgement both occur between reads.
        let pending = self.pending.pending_for_entity(entity_id).await?;
        let mut stored = self.stored.query_entity_history(entity_id, &unwindowed).await?.observations;
        let mut seen: HashSet<TemporalObservationIdentity> = stored.iter().filter_map(|observation| observation.identity).collect();
        for sequenced in pending {
            for observation in pending_observations(&sequenced, compiled) {
                let Some(identity) = observation.identity else {
                    continue;
                };
                if seen.insert(identity) {
                    stored.push(observation);
                }
            }
        }
        stored.sort_by(|left, right| {
            left.row
                .attribute_name
                .cmp(&right.row.attribute_name)
                .then_with(|| compare_dataset_ids(left.row.dataset_id.as_ref(), right.row.dataset_id.as_ref()))
                .then_with(|| temporal_position(&left.row, compiled.window.timeproperty).cmp(&temporal_position(&right.row, compiled.window.timeproperty)))
                .then_with(|| left.row.instance_id.as_str().cmp(right.row.instance_id.as_str()))
        });
        Ok(apply_last_n(stored, compiled))
    }
}

#[async_trait]
impl TemporalHistoryReader for PendingTemporalHistoryOverlay {
    fn capabilities(&self) -> TemporalReaderCapabilities {
        TemporalReaderCapabilities {
            aggregation: AggregationPushdown::None,
            history_visibility: HistoryVisibility::Immediate,
        }
    }

    async fn entity_exists(&self, entity_id: &Uri) -> Result<bool, TemporalError> {
        if !self.pending.pending_for_entity(entity_id).await?.is_empty() {
            return Ok(true);
        }
        self.stored.entity_exists(entity_id).await
    }

    async fn query_entity_ids(&self, compiled: &CompiledTemporalQuery) -> Result<TemporalEntityIdOutcome, TemporalError> {
        let pending = self.pending.pending_intents().await?;
        let mut unpaged = compiled.clone();
        unpaged.pagination = Pagination {
            limit: u32::MAX,
            offset: 0,
            count: CountPolicy::Omit,
        };
        let stored = self.stored.query_entity_ids(&unpaged).await?;
        let (stored_ids, _) = stored
            .complete(unpaged.pagination)
            .map_err(|error| TemporalError::internal(error.to_string()))?;
        let mut identifiers = stored_ids;
        let mut seen: HashSet<Uri> = identifiers.iter().cloned().collect();
        let selection = ResidualFilter::over(compiled.entities.clone());
        for sequenced in pending {
            if seen.contains(&sequenced.intent.entity_id) || !intent_matches_query(&sequenced, compiled, &selection) {
                continue;
            }
            seen.insert(sequenced.intent.entity_id.clone()); // clone: deduplication and the result both own the identifier.
            identifiers.push(sequenced.intent.entity_id.clone()); // clone: the result outlives the pending intent.
        }
        identifiers.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        Ok(TemporalEntityIdOutcome {
            candidates: identifiers
                .into_iter()
                .map(|entity_id| TemporalEntityCandidate {
                    entity_id,
                    selection_value: None,
                })
                .collect(),
            residual: TemporalResidual::default(),
            pagination: TemporalPaginationOutcome::Deferred,
        })
    }

    async fn query_entity_history(&self, entity_id: &Uri, compiled: &CompiledTemporalQuery) -> Result<TemporalHistoryOutcome, TemporalError> {
        Ok(TemporalHistoryOutcome {
            observations: self.complete_entity_history(entity_id, compiled).await?,
        })
    }

    async fn query_entity_aggregation(&self, _entity_id: &Uri, _compiled: &CompiledTemporalQuery) -> Result<Vec<TemporalAggregationRow>, TemporalError> {
        Err(TemporalError::operation_not_supported(
            "aggregation requires sink-side pending-intent union support",
        ))
    }
}

/// Return whether an intent contributes both entity selection and history rows.
fn intent_matches_query(sequenced: &SequencedTemporalAppendIntent, compiled: &CompiledTemporalQuery, selection: &ResidualFilter) -> bool {
    let selection_matches = sequenced
        .intent
        .selection_value
        .as_ref()
        .is_some_and(|value| residual_filter::matches(selection, value));
    selection_matches && !pending_observations(sequenced, compiled).is_empty()
}

/// Project matching history entries from one pending intent.
fn pending_observations(sequenced: &SequencedTemporalAppendIntent, compiled: &CompiledTemporalQuery) -> Vec<TemporalHistoryObservation> {
    sequenced
        .intent
        .history_entries()
        .filter_map(|(identity, entry)| pending_observation(identity, entry))
        .filter(|observation| matches_history_row(&observation.row, compiled))
        .collect()
}

/// Convert one pending history entry into a provenance-bearing observation.
fn pending_observation(identity: TemporalObservationIdentity, entry: &TemporalAppendEntry) -> Option<TemporalHistoryObservation> {
    let row = match entry {
        TemporalAppendEntry::Observation { row, .. } => row.clone(), // clone: the overlay result owns a neutral row while the durable intent remains reusable.
        TemporalAppendEntry::Tombstone {
            attribute_name,
            dataset_id,
            instance_id,
            recorded_at,
        } => TemporalHistoryRow {
            attribute_name: attribute_name.clone(), // clone: the overlay row owns its attribute identity.
            attribute_type: AttributeType::Property,
            dataset_id: dataset_id.clone(), // clone: the overlay row owns its dataset identity.
            value: TemporalValue::Tombstone,
            observed_at: None,
            time: *recorded_at,
            created_at: *recorded_at,
            modified_at: *recorded_at,
            instance_id: instance_id.clone(), // clone: the overlay row owns its instance identifier.
            metadata: None,
            partition_total: None,
        },
        TemporalAppendEntry::Scope { .. } => return None,
    };
    Some(TemporalHistoryObservation { row, identity: Some(identity) })
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
    if !matches_dataset(row, compiled) {
        return false;
    }
    if compiled.window.timeproperty == TimeProp::Deleted && !matches!(row.value, TemporalValue::Tombstone) {
        return false;
    }
    matches_interval(temporal_position(row, compiled.window.timeproperty), compiled.window.interval)
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

/// Select the row timestamp requested by the temporal query.
const fn temporal_position(row: &TemporalHistoryRow, property: TimeProp) -> chrono::DateTime<chrono::Utc> {
    match property {
        TimeProp::Observed => match row.observed_at {
            Some(observed_at) => observed_at,
            None => row.time,
        },
        TimeProp::Created | TimeProp::Modified | TimeProp::Deleted => row.time,
    }
}

/// Return whether a temporal position falls inside the half-open query interval.
fn matches_interval(position: chrono::DateTime<chrono::Utc>, interval: TimeInterval) -> bool {
    match interval {
        TimeInterval::Unbounded => true,
        TimeInterval::Before(instant) => position < instant,
        TimeInterval::After(instant) => position >= instant,
        TimeInterval::Between { from, to } => position >= from && position < to,
    }
}

/// Compare optional validated dataset identifiers without erasing their type.
fn compare_dataset_ids(left: Option<&Uri>, right: Option<&Uri>) -> std::cmp::Ordering {
    match (left, right) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (Some(left), Some(right)) => left.as_str().cmp(right.as_str()),
    }
}

/// Retain the last requested observations in every attribute/dataset partition.
fn apply_last_n(observations: Vec<TemporalHistoryObservation>, compiled: &CompiledTemporalQuery) -> Vec<TemporalHistoryObservation> {
    let Some(last_n) = compiled.instances.last_n else {
        return observations;
    };
    let limit = usize::try_from(last_n).unwrap_or(usize::MAX);
    let mut selected = Vec::with_capacity(observations.len());
    let mut partition_start = 0usize;
    while partition_start < observations.len() {
        let first = &observations[partition_start].row;
        let mut partition_end = partition_start + 1;
        while partition_end < observations.len()
            && observations[partition_end].row.attribute_name == first.attribute_name
            && observations[partition_end].row.dataset_id == first.dataset_id
        {
            partition_end += 1;
        }
        let retained_start = partition_start.max(partition_end.saturating_sub(limit));
        selected.extend(observations[retained_start..partition_end].iter().cloned()); // clone: selected rows must outlive the partition borrow.
        partition_start = partition_end;
    }
    selected
}
