//! Capability-refusal and page-continuation tests for temporal read policy.

use crate::{
    error::TemporalError,
    outcome::TemporalEntityIdOutcome,
    port::{
        history_reader::{
            AggregationPushdown,
            HistoryVisibility,
            RecordedAttributeShape,
            ShapeHolders,
            TemporalAggregationOutcome,
            TemporalHistoryOutcome,
            TemporalHistoryReader,
            TemporalReaderCapabilities,
        },
        read_scope::{ScopeReach, TemporalReadScope, UnrecordedHistory},
    },
    read::ensure_aggregation_supported,
    service::TemporalService,
    service_limits::TemporalServiceLimits,
    stub_broker::{StubBroker, limits, live_target, service_under, uri},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{
    aggr_method::AggrMethod,
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    entity_type_name::EntityTypeName,
    identifier::Identifier,
    instance_identity::InstanceIdentity,
    time::date_time::NgsiLdDateTime,
    time_rel::TimeRel,
};
use andromeda_ngsi_ld_model::temporal::{StreamExtent, TemporalAggregationRow, TemporalHistoryRow, TemporalValue, evolution_instance::EvolutionInstance};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    page_window::PageWindow,
    split_filter::LeafTruths,
    temporal::{compiled::CompiledTemporalQuery, continuation::TemporalContinuation, window::TimeInterval},
};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use std::{
    collections::{HashMap, HashSet},
    num::NonZeroU32,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

/// Reader that records any query invocation while refusing all aggregation.
struct RefusingReader {
    /// Number of backend query methods invoked.
    query_calls: AtomicUsize,
}

#[async_trait]
impl TemporalHistoryReader for RefusingReader {
    fn capabilities(&self) -> TemporalReaderCapabilities {
        TemporalReaderCapabilities {
            aggregation: AggregationPushdown::None,
            history_visibility: HistoryVisibility::Immediate,
        }
    }

    async fn read_scope<'a>(&self, target: &'a ContextTarget, _reach: ScopeReach<'_>) -> Result<TemporalReadScope<'a>, TemporalError> {
        Ok(TemporalReadScope::new(target, UnrecordedHistory::Absent))
    }

    async fn entity_exists(&self, _scope: &TemporalReadScope<'_>, _entity_id: &Identifier) -> Result<bool, TemporalError> {
        self.query_calls.fetch_add(1, Ordering::Relaxed);
        panic!("query must not execute")
    }

    async fn entity_types(
        &self,
        _scope: &TemporalReadScope<'_>,
        _entity_ids: &[Identifier],
    ) -> Result<HashMap<Identifier, Vec<EntityTypeName>>, TemporalError> {
        self.query_calls.fetch_add(1, Ordering::Relaxed);
        panic!("query must not execute")
    }

    async fn query_entity_ids(
        &self,
        _scope: &TemporalReadScope<'_>,
        _compiled: &CompiledTemporalQuery,
        _window: &PageWindow,
        _count: CountPolicy,
    ) -> Result<TemporalEntityIdOutcome, TemporalError> {
        self.query_calls.fetch_add(1, Ordering::Relaxed);
        panic!("query must not execute")
    }

    async fn filter_leaf_truths(
        &self,
        _scope: &TemporalReadScope<'_>,
        _compiled: &CompiledTemporalQuery,
        _entity_ids: &[Identifier],
    ) -> Result<HashMap<Identifier, LeafTruths>, TemporalError> {
        self.query_calls.fetch_add(1, Ordering::Relaxed);
        panic!("query must not execute")
    }

    async fn in_effect_instances(
        &self,
        _scope: &TemporalReadScope<'_>,
        _compiled: &CompiledTemporalQuery,
        _entity_ids: &[Identifier],
    ) -> Result<HashMap<Identifier, Vec<TemporalHistoryRow>>, TemporalError> {
        self.query_calls.fetch_add(1, Ordering::Relaxed);
        panic!("query must not execute")
    }

    async fn window_relationship_instances(
        &self,
        _scope: &TemporalReadScope<'_>,
        _compiled: &CompiledTemporalQuery,
        _entity_ids: &[Identifier],
    ) -> Result<HashMap<Identifier, Vec<TemporalHistoryRow>>, TemporalError> {
        self.query_calls.fetch_add(1, Ordering::Relaxed);
        panic!("query must not execute")
    }

    async fn query_entity_history(
        &self,
        _scope: &TemporalReadScope<'_>,
        _entity_id: &Identifier,
        _compiled: &CompiledTemporalQuery,
    ) -> Result<TemporalHistoryOutcome, TemporalError> {
        self.query_calls.fetch_add(1, Ordering::Relaxed);
        panic!("query must not execute")
    }

    async fn query_entity_aggregation(
        &self,
        _scope: &TemporalReadScope<'_>,
        _entity_id: &Identifier,
        _compiled: &CompiledTemporalQuery,
    ) -> Result<TemporalAggregationOutcome, TemporalError> {
        self.query_calls.fetch_add(1, Ordering::Relaxed);
        panic!("query must not execute")
    }

    async fn query_merged_entity_aggregation(
        &self,
        _scope: &TemporalReadScope<'_>,
        _entity_id: &Identifier,
        _entity_types: &[EntityTypeName],
        _compiled: &CompiledTemporalQuery,
        _answered: &[TemporalHistoryRow],
    ) -> Result<Vec<TemporalAggregationRow>, TemporalError> {
        self.query_calls.fetch_add(1, Ordering::Relaxed);
        panic!("query must not execute")
    }

    async fn recorded_attribute_shapes(
        &self,
        _scope: &TemporalReadScope<'_>,
        _attributes: &[AttributeName],
    ) -> Result<HashMap<AttributeName, HashSet<RecordedAttributeShape>>, TemporalError> {
        self.query_calls.fetch_add(1, Ordering::Relaxed);
        panic!("query must not execute")
    }

    async fn held_attribute_shapes(
        &self,
        _scope: &TemporalReadScope<'_>,
        _holders: ShapeHolders<'_>,
        _compiled: &CompiledTemporalQuery,
        _shapes: &HashMap<AttributeName, HashSet<RecordedAttributeShape>>,
    ) -> Result<HashMap<AttributeName, HashSet<RecordedAttributeShape>>, TemporalError> {
        self.query_calls.fetch_add(1, Ordering::Relaxed);
        panic!("query must not execute")
    }
}

#[test]
fn aggregation_refusal_precedes_every_reader_query() {
    let reader = RefusingReader {
        query_calls: AtomicUsize::new(0),
    };
    let error = ensure_aggregation_supported(&reader, &[AggrMethod::Avg]).expect_err("reader must refuse aggregation");
    assert!(matches!(error, TemporalError::OperationNotSupported { .. }));
    pretty_assertions::assert_eq!(reader.query_calls.load(Ordering::Relaxed), 0);
}

fn minute(offset: i64) -> DateTime<Utc> {
    "2020-01-01T01:00:00Z".parse::<NgsiLdDateTime>().expect("a valid instant").get() + Duration::minutes(offset)
}

/// The instances of `attribute` observed at `minute(offset)` for each
/// offset, in a stream read to `extent`.
fn observed(attribute: &str, offsets: impl IntoIterator<Item = i64>, extent: StreamExtent) -> Vec<TemporalHistoryRow> {
    offsets
        .into_iter()
        .map(|offset| TemporalHistoryRow {
            attribute_name: format!("https://example.com/{attribute}").parse().expect("a valid attribute name"),
            attribute_type: AttributeType::Property,
            dataset_id: None,
            value: TemporalValue::Number {
                number: JsonNumber::from(offset),
                datatype: None,
            },
            observed_at: Some(minute(offset)),
            recorded_at: minute(offset),
            created_at: Some(minute(offset)),
            modified_at: Some(minute(offset)),
            instance: EvolutionInstance::Attribute(InstanceIdentity::Verbatim(uri(&format!("urn:ngsi-ld:Instance:{attribute}:{offset}")))),
            metadata: None,
            stream_extent: extent,
        })
        .collect()
}

/// A service paging at 19 instances and answering a `lastN` of up to 19 in
/// full, whose one entity reads as `rows`.
fn paging_service(rows: Vec<TemporalHistoryRow>) -> TemporalService {
    let limits = limits();
    let mut instance_limits = limits.instance_limits;
    instance_limits.default_page = 19;
    instance_limits.maximum = 19;
    service_under(
        Arc::new(StubBroker::new([]).paging(uri("urn:ngsi-ld:Vehicle:1"), rows)),
        None,
        TemporalServiceLimits { instance_limits, ..limits },
    )
}

fn after_2020(last_n: Option<u32>) -> TemporalQueryParameters {
    TemporalQueryParameters {
        timerel: Some(TimeRel::After),
        time_at: Some("2020-01-01T00:00:00Z".parse().expect("a valid instant")),
        last_n: last_n.and_then(NonZeroU32::new),
        ..TemporalQueryParameters::default()
    }
}

/// Clause 6.3.10: a forward page ends on the most recent instant it served,
/// whole, so its continuation opens one microsecond past it.
#[tokio::test]
async fn a_forward_cut_retrieve_continues_one_microsecond_past_its_edge() {
    let service = paging_service(observed("speed", 1..=19, StreamExtent::Cut));
    let result = service
        .get(&live_target(), &uri("urn:ngsi-ld:Vehicle:1"), &after_2020(None), &ContextSpec::core_only())
        .await
        .expect("a readable history");
    let range = result.range.expect("a partial response");
    pretty_assertions::assert_eq!(
        range.next,
        Some(TemporalContinuation {
            interval: TimeInterval::After(minute(19) + Duration::microseconds(1)),
            last_n: None,
            period_origin: None,
        })
    );
}

/// A `lastN` of 50 over a maximum of 19 pages backwards: `speed` was cut at
/// 01:02, `fuelLevel` received its 6 instances whole, so the continuation
/// asks for the 44 the sparser stream is still owed, before 01:02.
#[tokio::test]
async fn a_backward_cut_retrieve_continues_with_the_last_n_its_sparsest_stream_is_owed() {
    let mut rows = observed("speed", 2..=20, StreamExtent::Cut);
    rows.extend(observed("fuelLevel", 15..=20, StreamExtent::Whole));
    let service = paging_service(rows);
    let result = service
        .get(&live_target(), &uri("urn:ngsi-ld:Vehicle:1"), &after_2020(Some(50)), &ContextSpec::core_only())
        .await
        .expect("a readable history");
    let range = result.range.expect("a partial response");
    pretty_assertions::assert_eq!(
        range.next,
        Some(TemporalContinuation {
            interval: TimeInterval::Between {
                from: "2020-01-01T00:00:00Z".parse().expect("a valid instant"),
                to: minute(2),
            },
            last_n: NonZeroU32::new(44),
            period_origin: None,
        })
    );
}
