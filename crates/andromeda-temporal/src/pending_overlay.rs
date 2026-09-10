//! Immediate logical temporal reads over a deferred history sink.

use crate::{
    append_intent::{SequencedTemporalAppendIntent, TemporalAppendEntry, TemporalObservationIdentity},
    error::TemporalError,
    outcome::{TemporalEntityCandidate, TemporalEntityIdOutcome, TemporalPaginationOutcome, TemporalResidual},
    port::{
        history_reader::{
            AggregationPushdown,
            HistoryVisibility,
            RecordedAttributeShape,
            TemporalHistoryObservation,
            TemporalHistoryOutcome,
            TemporalHistoryReader,
            TemporalReaderCapabilities,
        },
        pending_intent_reader::PendingTemporalIntentReader,
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{query_residual::ResidualFilter, residual_filter};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    entity_type_name::EntityTypeName,
    instance_family::InstanceFamily,
    time_prop::TimeProp,
    uri::Uri,
};
use andromeda_ngsi_ld_model::temporal::{TemporalAggregationRow, TemporalHistoryRow, TemporalValue};
use andromeda_ngsi_ld_query::{
    dataset::DatasetFilter,
    ordering::sort_direction::SortDirection,
    pagination::{CountPolicy, Pagination},
    temporal::{compiled::CompiledTemporalQuery, ordering::TemporalEntityOrdering, window::TimeInterval},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// Which way the merged stage-one identifiers run.
///
/// Clause 5.7.4.3 admits no ordering member but `id`, so the whole of what an
/// ordering can ask of this merge is a direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MergedIdentifierOrder {
    /// Identifiers ascending: what an `id;asc` ordering asks for, and also the
    /// store's own stable order when the request asked for no ordering at all
    /// (clause 5.5.9).
    Ascending,
    /// Identifiers descending, which only an explicit `id;desc` asks for.
    Descending,
}

impl MergedIdentifierOrder {
    /// The order the identifiers of a compiled temporal query are merged in.
    const fn of_query(ordering: Option<&TemporalEntityOrdering>) -> MergedIdentifierOrder {
        match ordering {
            Some(TemporalEntityOrdering {
                direction: SortDirection::Descending,
                ..
            }) => MergedIdentifierOrder::Descending,
            Some(TemporalEntityOrdering {
                direction: SortDirection::Ascending,
                ..
            })
            | None => MergedIdentifierOrder::Ascending,
        }
    }
}

/// Put the merged stage-one identifiers in the order the response renders in.
///
/// Clause 5.7.4.5: "If Entity ordering is specified (see clause 4.23), then the
/// JSON-LD array returned shall be ordered according to the member names
/// specified." The clause makes no exception for an Entity whose history is
/// still an unapplied intent, so the pending identifiers are ordered together
/// with the stored ones rather than appended after them. With no ordering
/// requested the same ascending sequence is what clause 5.5.9 needs for a
/// stable page.
///
/// The comparison is over the identifier text alone. An ordering may name an
/// ICU collation (clause 4.23.1), which refines the comparison inside the store
/// because the store holds a comparator for it; this process holds none, so a
/// collation cannot change where an identifier lands here. That refinement is
/// the one thing the overlay does not reproduce, and it is a difference between
/// two distinct URIs rather than between an ordered and an unordered result.
fn order_merged_identifiers(identifiers: &mut [Uri], order: MergedIdentifierOrder) {
    match order {
        MergedIdentifierOrder::Ascending => identifiers.sort_by(|left, right| left.as_str().cmp(right.as_str())),
        MergedIdentifierOrder::Descending => identifiers.sort_by(|left, right| right.as_str().cmp(left.as_str())),
    }
}

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
    async fn complete_entity_history(
        &self,
        target: &ContextTarget,
        entity_id: &Uri,
        compiled: &CompiledTemporalQuery,
    ) -> Result<Vec<TemporalHistoryObservation>, TemporalError> {
        let mut unwindowed = compiled.clone();
        unwindowed.instances.last_n = None;
        // Snapshot anchor intents first. If delivery transitions after this read,
        // the subsequent sink read either sees the accepted rows or the retained
        // intent still supplies them. Reading in the opposite order creates a
        // gap when acceptance and acknowledgement both occur between reads.
        let pending = self.pending.pending_for_entity(target, entity_id).await?;
        let mut stored = self.stored.query_entity_history(target, entity_id, &unwindowed).await?.observations;
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
                .then_with(|| {
                    left.row
                        .time_axis(compiled.window.timeproperty)
                        .cmp(&right.row.time_axis(compiled.window.timeproperty))
                })
                // Rows the selected axis does not position are still ordered,
                // and the write clock is the only total order every row has.
                .then_with(|| left.row.recorded_at.cmp(&right.row.recorded_at))
                .then_with(|| rendered_instance_id(&left.row).cmp(&rendered_instance_id(&right.row)))
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

    async fn entity_exists(&self, target: &ContextTarget, entity_id: &Uri) -> Result<bool, TemporalError> {
        if !self.pending.pending_for_entity(target, entity_id).await?.is_empty() {
            return Ok(true);
        }
        self.stored.entity_exists(target, entity_id).await
    }

    async fn recorded_attribute_shapes(
        &self,
        target: &ContextTarget,
        attributes: &[AttributeName],
    ) -> Result<HashMap<AttributeName, HashSet<RecordedAttributeShape>>, TemporalError> {
        // The pending queue is indexed by entity, not by attribute, so the
        // shapes an attribute has been recorded with are read from the durable
        // history alone; an intent that has not been drained yet contributes
        // once it lands.
        self.stored.recorded_attribute_shapes(target, attributes).await
    }

    async fn entity_types(&self, target: &ContextTarget, entity_ids: &[Uri]) -> Result<HashMap<Uri, Vec<EntityTypeName>>, TemporalError> {
        let mut types_by_entity = self.stored.entity_types(target, entity_ids).await?;
        // An entity whose first write has not been drained yet has no catalogue
        // row to read its types from, so the pending intent that will create it
        // supplies the types its write stated.
        for entity_id in entity_ids {
            if types_by_entity.contains_key(entity_id) {
                continue;
            }
            let pending_types = self
                .pending
                .pending_for_entity(target, entity_id)
                .await?
                .into_iter()
                .map(|sequenced| sequenced.intent.entity_types)
                .find(|types| !types.is_empty());
            if let Some(types) = pending_types {
                types_by_entity.insert(entity_id.clone(), types); // clone: the index owns its validated entity URI.
            }
        }
        Ok(types_by_entity)
    }

    async fn query_entity_ids(&self, target: &ContextTarget, compiled: &CompiledTemporalQuery) -> Result<TemporalEntityIdOutcome, TemporalError> {
        let pending = self.pending.pending_intents(target).await?;
        let mut unpaged = compiled.clone();
        unpaged.pagination = Pagination {
            limit: u32::MAX,
            offset: 0,
            count: CountPolicy::Omit,
        };
        let stored = self.stored.query_entity_ids(target, &unpaged).await?;
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
        order_merged_identifiers(&mut identifiers, MergedIdentifierOrder::of_query(compiled.ordering.as_ref()));
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

    async fn query_entity_history(
        &self,
        target: &ContextTarget,
        entity_id: &Uri,
        compiled: &CompiledTemporalQuery,
    ) -> Result<TemporalHistoryOutcome, TemporalError> {
        Ok(TemporalHistoryOutcome {
            observations: self.complete_entity_history(target, entity_id, compiled).await?,
        })
    }

    async fn query_entity_aggregation(
        &self,
        _target: &ContextTarget,
        _entity_id: &Uri,
        _compiled: &CompiledTemporalQuery,
    ) -> Result<Vec<TemporalAggregationRow>, TemporalError> {
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
        .filter_map(|(identity, entry)| pending_observation(identity, entry, sequenced.intent.record.recorded_at()))
        .filter(|observation| matches_history_row(&observation.row, compiled))
        .collect()
}

/// Convert one pending history entry into a provenance-bearing observation.
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
    if compiled.window.timeproperty == TimeProp::Deleted && !matches!(row.value, TemporalValue::Tombstone { .. }) {
        return false;
    }
    matches_interval(row.time_axis(compiled.window.timeproperty), compiled.window.interval)
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

/// The identifier a client would see for one pending row, used only to order rows.
fn rendered_instance_id(row: &TemporalHistoryRow) -> String {
    row.instance.rendered(InstanceFamily::of_attribute_name(&row.attribute_name)).to_string()
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

/// Compare optional validated dataset identifiers without erasing their type.
fn compare_dataset_ids(left: Option<&Uri>, right: Option<&Uri>) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
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

#[cfg(test)]
mod tests {
    use crate::pending_overlay::{MergedIdentifierOrder, order_merged_identifiers};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_query::{
        ordering::{collation::IcuCollation, sort_direction::SortDirection},
        temporal::ordering::TemporalEntityOrdering,
    };

    fn ids(raw: &[&str]) -> Vec<Uri> {
        raw.iter().map(|id| id.parse().expect("valid URI")).collect()
    }

    fn spelled(identifiers: &[Uri]) -> Vec<&str> {
        identifiers.iter().map(Uri::as_str).collect()
    }

    /// With no clause-4.23 ordering the store returns `id` ascending under the
    /// identifier column's byte-wise collation, and the merged union has to sit
    /// in that same order for clause 5.5.9 pagination to be stable.
    #[test]
    fn an_unordered_query_merges_the_pending_identifiers_into_the_ascending_order() {
        let mut identifiers = ids(&["urn:ngsi-ld:T:c", "urn:ngsi-ld:T:a", "urn:ngsi-ld:T:b"]);
        order_merged_identifiers(&mut identifiers, MergedIdentifierOrder::of_query(None));
        assert_eq!(spelled(&identifiers), vec!["urn:ngsi-ld:T:a", "urn:ngsi-ld:T:b", "urn:ngsi-ld:T:c"]);
    }

    /// Clause 5.7.4.5: "If Entity ordering is specified (see clause 4.23), then
    /// the JSON-LD array returned shall be ordered according to the member
    /// names specified." An Entity whose history is still an unapplied intent
    /// is no exception, so it takes its place in the sequence rather than being
    /// appended after it.
    #[test]
    fn an_ascending_ordering_places_the_pending_identifier_rather_than_appending_it() {
        let ordering = TemporalEntityOrdering::builder()
            .direction(SortDirection::Ascending)
            .collation(Some(IcuCollation::parse("de-u-co-phonebk").expect("valid collation")))
            .build();
        let mut identifiers = ids(&["urn:ngsi-ld:T:a", "urn:ngsi-ld:T:c", "urn:ngsi-ld:T:b"]);
        order_merged_identifiers(&mut identifiers, MergedIdentifierOrder::of_query(Some(&ordering)));
        assert_eq!(spelled(&identifiers), vec!["urn:ngsi-ld:T:a", "urn:ngsi-ld:T:b", "urn:ngsi-ld:T:c"]);
    }

    /// A descending ordering runs the other way, and the pending identifier is
    /// placed by the same comparison rather than left at the end.
    #[test]
    fn a_descending_ordering_places_the_pending_identifier_by_the_same_comparison() {
        let ordering = TemporalEntityOrdering::builder().direction(SortDirection::Descending).build();
        let mut identifiers = ids(&["urn:ngsi-ld:T:c", "urn:ngsi-ld:T:a", "urn:ngsi-ld:T:b"]);
        order_merged_identifiers(&mut identifiers, MergedIdentifierOrder::of_query(Some(&ordering)));
        assert_eq!(spelled(&identifiers), vec!["urn:ngsi-ld:T:c", "urn:ngsi-ld:T:b", "urn:ngsi-ld:T:a"]);
    }

    /// Only the direction reaches the merge: clause 5.7.4.3 admits no ordering
    /// member but `id`, and the ICU collation refines a comparison the store
    /// makes rather than this one.
    #[test]
    fn the_merge_order_reads_the_direction_alone() {
        let collated = TemporalEntityOrdering::builder()
            .direction(SortDirection::Ascending)
            .collation(Some(IcuCollation::parse("de-u-co-phonebk").expect("valid collation")))
            .build();
        pretty_assertions::assert_eq!(MergedIdentifierOrder::of_query(Some(&collated)), MergedIdentifierOrder::Ascending);
        pretty_assertions::assert_eq!(MergedIdentifierOrder::of_query(None), MergedIdentifierOrder::Ascending);
    }
}
