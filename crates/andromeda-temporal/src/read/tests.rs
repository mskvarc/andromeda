//! Page-continuation tests for temporal read policy.

use crate::{
    service::TemporalService,
    service_limits::TemporalServiceLimits,
    stub_broker::{StubBroker, admitted, limits, live_target, service_under, uri},
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, instance_identity::InstanceIdentity, time::date_time::NgsiLdDateTime, time_rel::TimeRel};
use andromeda_ngsi_ld_model::temporal::{StreamExtent, TemporalHistoryRow, TemporalValue, evolution_instance::EvolutionInstance};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_query::temporal::{continuation::TemporalContinuation, window::TimeInterval};
use andromeda_ngsi_ld_request::query::temporal::{TemporalQueryParameters, admitted::AdmittedTemporalRequest};
use chrono::{DateTime, Duration, Utc};
use std::{num::NonZeroU32, sync::Arc};

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

fn after_2020(last_n: Option<u32>) -> AdmittedTemporalRequest {
    admitted(TemporalQueryParameters {
        timerel: Some(TimeRel::After),
        time_at: Some("2020-01-01T00:00:00Z".parse().expect("a valid instant")),
        last_n: last_n.and_then(NonZeroU32::new),
        ..TemporalQueryParameters::default()
    })
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
        })
    );
}
