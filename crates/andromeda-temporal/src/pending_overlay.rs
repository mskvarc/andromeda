//! Immediate logical temporal reads over a deferred history sink.

/// The order the stage-one identifiers of stored and pending entities are merged in.
mod merged_identifier_order;

use crate::{
    append_intent::{KeyedTemporalAppendIntent, TemporalObservationIdentity},
    error::TemporalError,
    instance_page_selection::select_page,
    outcome::{TemporalEntityCandidate, TemporalEntityIdOutcome, TemporalPaginationOutcome, TemporalResidual},
    pending_observation::{intent_matches_query, is_scope_row, pending_observations},
    pending_overlay::merged_identifier_order::{MergedIdentifierOrder, order_merged_identifiers},
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
    temporal_invariant::TemporalInvariant,
    temporal_operation_unsupported::TemporalOperationUnsupported,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::query_residual::ResidualFilter;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier, instance_family::InstanceFamily};
use andromeda_ngsi_ld_model::temporal::{TemporalAggregationRow, TemporalHistoryRow};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    entity_selection::EntitySelection,
    page_window::PageWindow,
    split_filter::{LeafTruths, SplitFilter},
    temporal::{compiled::CompiledTemporalQuery, instance_page::InstancePage, window::TimeInterval},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// The intents still pending on `target`, which `read` fetches from the
/// pending queue only when `target` is the live branch.
///
/// Only the live branch defers its history. A Snapshot branch records it in
/// the write's own transaction, so none of its history is ever pending, and
/// the queue -- which holds the live branch's intents alone -- is not asked:
/// what it holds would be the live branch's history, not the Snapshot's.
async fn pending_on_branch<Read, Pending>(target: &ContextTarget, read: Read) -> Result<Vec<KeyedTemporalAppendIntent>, TemporalError>
where
    Read: FnOnce() -> Pending,
    Pending: Future<Output = Result<Vec<KeyedTemporalAppendIntent>, TemporalError>>,
{
    match target {
        ContextTarget::Live => read().await,
        ContextTarget::Snapshot(_) => Ok(Vec::new()),
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

    /// The intents still pending for `entity_id` in `target`.
    async fn pending_for_entity(&self, target: &ContextTarget, entity_id: &Identifier) -> Result<Vec<KeyedTemporalAppendIntent>, TemporalError> {
        pending_on_branch(target, || self.pending.pending_for_entity(entity_id)).await
    }

    /// Read all matching rows before the overlay narrows the union to its page.
    async fn complete_entity_history(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        compiled: &CompiledTemporalQuery,
    ) -> Result<Vec<TemporalHistoryObservation>, TemporalError> {
        let mut unwindowed = compiled.clone();
        unwindowed.instances.page = InstancePage::Whole;
        // Snapshot anchor intents first. If delivery transitions after this read,
        // the subsequent sink read either sees the accepted rows or the retained
        // intent still supplies them. Reading in the opposite order creates a
        // gap when acceptance and acknowledgement both occur between reads.
        let pending = self.pending_for_entity(target, entity_id).await?;
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
        retain_valid_scope_events(&mut stored, compiled);
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
        Ok(select_page(stored, compiled))
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

    async fn entity_exists(&self, target: &ContextTarget, entity_id: &Identifier) -> Result<bool, TemporalError> {
        if !self.pending_for_entity(target, entity_id).await?.is_empty() {
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

    async fn entity_types(&self, target: &ContextTarget, entity_ids: &[Identifier]) -> Result<HashMap<Identifier, Vec<EntityTypeName>>, TemporalError> {
        let mut types_by_entity = self.stored.entity_types(target, entity_ids).await?;
        // An entity whose first write has not been drained yet has no catalogue
        // row to read its types from, so the pending intent that will create it
        // supplies the types its write stated.
        for entity_id in entity_ids {
            if types_by_entity.contains_key(entity_id) {
                continue;
            }
            let pending_types = self
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

    /// The stored and the pending identifiers form one collection, so neither
    /// can be paged on its own: every stored match is read, the pending ones
    /// merged in, and the whole set handed back in the query's order for the
    /// caller to count and cut `_window` from (clauses 5.5.9 and 6.3.13).
    async fn query_entity_ids(
        &self,
        target: &ContextTarget,
        compiled: &CompiledTemporalQuery,
        _window: &PageWindow,
        _count: CountPolicy,
    ) -> Result<TemporalEntityIdOutcome, TemporalError> {
        let pending = pending_on_branch(target, || self.pending.pending_intents()).await?;
        let unpaged = PageWindow::unbounded();
        let stored = self.stored.query_entity_ids(target, compiled, &unpaged, CountPolicy::Omit).await?;
        let (stored_ids, _) = stored
            .complete(&unpaged, CountPolicy::Omit)
            .map_err(|error| TemporalError::from(TemporalInvariant::IncompleteOutcome(error)))?;
        let mut identifiers = stored_ids;
        let mut seen: HashSet<Identifier> = identifiers.iter().cloned().collect();
        let selection = ResidualFilter::over(compiled.entities.clone());
        let contributed_selection = ResidualFilter::over(without_split_filters(&compiled.entities));
        for sequenced in pending {
            // Clause 5.7.4.4: an entity a Context Source contributed to is
            // judged on its gathered parts, so its pending part is held to the
            // selection the stored statement holds it to, without the value,
            // geo and scope filters.
            let applied = if compiled.contributed.contains(&sequenced.intent.entity_id) {
                &contributed_selection
            } else {
                &selection
            };
            if seen.contains(&sequenced.intent.entity_id) || !intent_matches_query(&sequenced, compiled, applied) {
                continue;
            }
            seen.insert(sequenced.intent.entity_id.clone()); // clone: deduplication and the result both own the identifier.
            identifiers.push(sequenced.intent.entity_id.clone()); // clone: the result outlives the pending intent.
        }
        order_merged_identifiers(&mut identifiers, MergedIdentifierOrder::of_query(compiled.ordering.as_ref()));
        Ok(TemporalEntityIdOutcome {
            candidates: identifiers
                .into_iter()
                .map(|entity_id| TemporalEntityCandidate { entity_id, selection: None })
                .collect(),
            residual: TemporalResidual::default(),
            pagination: TemporalPaginationOutcome::Deferred,
        })
    }

    /// The stored part's truths, joined with the part each pending intent of
    /// the entity contributes inside the window: an intent the sink has not
    /// applied yet is still part of the entity the filters are judged on.
    async fn filter_leaf_truths(
        &self,
        target: &ContextTarget,
        compiled: &CompiledTemporalQuery,
        entity_ids: &[Identifier],
    ) -> Result<HashMap<Identifier, LeafTruths>, TemporalError> {
        let filter = SplitFilter::of(&compiled.entities);
        if filter.is_empty() {
            return Ok(HashMap::new());
        }
        let mut truths = self.stored.filter_leaf_truths(target, compiled, entity_ids).await?;
        for entity_id in entity_ids {
            for sequenced in self.pending_for_entity(target, entity_id).await? {
                let Some(observed) = sequenced.intent.selection_value.as_ref() else {
                    continue;
                };
                if pending_observations(&sequenced, compiled).is_empty() {
                    continue;
                }
                truths
                    .entry(entity_id.clone()) // clone: the map owns its validated entity URI.
                    .or_insert_with(|| LeafTruths::none(filter.leaf_count()))
                    .or(&observed.leaf_truths(&filter));
            }
        }
        Ok(truths)
    }

    async fn query_entity_history(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        compiled: &CompiledTemporalQuery,
    ) -> Result<TemporalHistoryOutcome, TemporalError> {
        Ok(TemporalHistoryOutcome {
            observations: self.complete_entity_history(target, entity_id, compiled).await?,
        })
    }

    async fn query_merged_entity_aggregation(
        &self,
        _target: &ContextTarget,
        _entity_id: &Identifier,
        _entity_types: &[EntityTypeName],
        _compiled: &CompiledTemporalQuery,
        _answered: &[TemporalHistoryRow],
    ) -> Result<Vec<TemporalAggregationRow>, TemporalError> {
        Err(TemporalError::OperationNotSupported(TemporalOperationUnsupported::PendingUnionUnsupported))
    }

    async fn query_entity_aggregation(
        &self,
        _target: &ContextTarget,
        _entity_id: &Identifier,
        _compiled: &CompiledTemporalQuery,
    ) -> Result<Vec<TemporalAggregationRow>, TemporalError> {
        Err(TemporalError::OperationNotSupported(TemporalOperationUnsupported::PendingUnionUnsupported))
    }
}

/// `selection` without the value, geo and scope filters clause 5.7.4.4
/// applies to a split entity only once its parts are gathered.
fn without_split_filters(selection: &EntitySelection) -> EntitySelection {
    let mut stripped = selection.clone(); // clone: the query keeps its filters for every entity that is not contributed
    stripped.q_expr = None;
    stripped.geo = None;
    stripped.scope_q = None;
    stripped
}

/// The instant a window opens at, when it opens at one.
const fn window_start(interval: TimeInterval) -> Option<DateTime<Utc>> {
    match interval {
        TimeInterval::After(from) | TimeInterval::Between { from, .. } => Some(from),
        TimeInterval::Before(_) | TimeInterval::Unbounded => None,
    }
}

/// Drop the Scope events a later one had already superseded when the window
/// opened.
///
/// Clause 4.18 ends a Scope's validity at the next scope event of the same
/// Entity, and the stored read cannot see an intent the sink has not applied
/// yet: for the newest event it knows, it reports an interval running to the
/// present. A pending event closes that interval at its own instant, so an
/// event whose interval now ends at or before the window opened is no longer
/// part of the answer. Only the last event at or before that instant survives,
/// which is the same rule the stored statement applies to the events it can
/// see.
fn retain_valid_scope_events(observations: &mut Vec<TemporalHistoryObservation>, compiled: &CompiledTemporalQuery) {
    let Some(from) = window_start(compiled.window.interval) else {
        return;
    };
    let timeproperty = compiled.window.timeproperty;
    let in_force = observations
        .iter()
        .filter(|observation| is_scope_row(&observation.row))
        .filter_map(|observation| observation.row.time_axis(timeproperty))
        .filter(|instant| *instant <= from)
        .max();
    let Some(in_force) = in_force else {
        return;
    };
    observations.retain(|observation| !is_scope_row(&observation.row) || observation.row.time_axis(timeproperty).is_none_or(|instant| instant >= in_force));
}

/// The identifier a client would see for one pending row, used only to order rows.
fn rendered_instance_id(row: &TemporalHistoryRow) -> String {
    row.instance.rendered(InstanceFamily::of_attribute_name(&row.attribute_name)).to_string()
}

/// Compare optional validated dataset identifiers without erasing their type.
fn compare_dataset_ids(left: Option<&Identifier>, right: Option<&Identifier>) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(left), Some(right)) => left.as_str().cmp(right.as_str()),
    }
}
