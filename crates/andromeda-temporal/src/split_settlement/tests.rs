use crate::{
    answered_fixture::answered,
    local_in_effect::LocalInEffect,
    query_reading::QueryReading,
    split_settlement::AnsweredParts,
    stub_broker::{StubBroker, live_target, service, uri},
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    identifier::Identifier,
    instance_identity::InstanceIdentity,
    time::date_time::NgsiLdDateTime,
    time_prop::TimeProp,
    time_rel::TimeRel,
};
use andromeda_ngsi_ld_model::temporal::{
    StreamExtent,
    TemporalHistoryRow,
    TemporalValue,
    evolution_instance::EvolutionInstance,
    temporal_document::TemporalDocument,
};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_query::{filter::language_targeting::LanguageTargeting, split_filter::LeafTruths};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

const VEHICLE: &str = "urn:ngsi-ld:Vehicle:1";
const PARKING: &str = "urn:ngsi-ld:Parking:1";

fn at(raw: &str) -> DateTime<Utc> {
    raw.parse::<NgsiLdDateTime>().expect("a valid instant").get()
}

/// A split query over the window from 12:00 to 13:00 on `observedAt`,
/// filtering with `q`, every name a full IRI.
fn split_query(q: &str) -> TemporalQueryParameters {
    TemporalQueryParameters {
        entity_type: Some("https://example.com/Vehicle".parse().expect("a valid type")),
        q: Some(q.parse().expect("a valid q")),
        timerel: Some(TimeRel::Between),
        time_at: Some(at("2020-08-01T12:00:00Z")),
        end_time_at: Some(at("2020-08-01T13:00:00Z")),
        timeproperty: Some(TimeProp::Observed),
        ..TemporalQueryParameters::default()
    }
}

/// One row of `attribute` holding `value`, observed at `observed`.
fn row(attribute: &str, family: AttributeType, value: TemporalValue, observed: &str) -> TemporalHistoryRow {
    let instant = at(observed);
    TemporalHistoryRow {
        attribute_name: attribute.parse::<AttributeName>().expect("a valid attribute IRI"),
        attribute_type: family,
        dataset_id: None,
        value,
        observed_at: Some(instant),
        recorded_at: instant,
        created_at: Some(instant),
        modified_at: Some(instant),
        instance: EvolutionInstance::Attribute(InstanceIdentity::Verbatim(uri(&format!("urn:ngsi-ld:Instance:{attribute}:{observed}")))),
        metadata: None,
        stream_extent: StreamExtent::Whole,
    }
}

fn number(value: f64) -> TemporalValue {
    TemporalValue::Number {
        number: JsonNumber::try_from(value).expect("a finite number"),
        datatype: None,
    }
}

fn speed(value: f64, observed: &str) -> TemporalHistoryRow {
    row("https://example.com/speed", AttributeType::Property, number(value), observed)
}

fn free(value: f64, observed: &str) -> TemporalHistoryRow {
    row("https://example.com/free", AttributeType::Property, number(value), observed)
}

fn parked_in(observed: &str) -> TemporalHistoryRow {
    row(
        "https://example.com/parkedIn",
        AttributeType::Relationship,
        TemporalValue::Uri(uri(PARKING).as_uri().clone()), // clone: the row owns the object it names
        observed,
    )
}

/// The part of Vehicle 1 a source answered with, holding `members`.
async fn vehicle_part(members: Value) -> TemporalDocument {
    let mut answer = json!({"id": VEHICLE, "type": "Vehicle"});
    if let (Value::Object(entity), Value::Object(members)) = (&mut answer, members) {
        entity.extend(members);
    }
    answered(&answer).await
}

/// Which contributed entities `broker` and the answered parts keep under `q`.
async fn retained(broker: StubBroker, q: &str, in_window: &[&TemporalDocument], in_effect: &[&TemporalDocument], local: &LocalInEffect) -> Vec<Identifier> {
    let service = service(broker);
    let parts = AnsweredParts {
        in_window: in_window.to_vec(),
        in_effect: in_effect.to_vec(),
    };
    let settlement = service
        .settle_split_filters(
            &live_target(),
            &split_query(q),
            QueryReading {
                context: &ContextSpec::none(),
                language_targeting: LanguageTargeting::Specification,
            },
            &parts,
            local,
        )
        .await
        .expect("the settlement runs");
    settlement.retained.into_iter().collect()
}

/// What `broker` holds in effect of Vehicle 1, read as the settlement reads it.
async fn local_in_effect(broker: StubBroker, q: &str) -> LocalInEffect {
    service(broker)
        .read_local_in_effect(
            &live_target(),
            &split_query(q),
            QueryReading {
                context: &ContextSpec::none(),
                language_targeting: LanguageTargeting::Specification,
            },
            &[uri(VEHICLE)],
        )
        .await
        .expect("the local part reads")
}

const ABOVE_50: &str = "\"https://example.com/speed\">50";

/// Clause 4.11 with clause 5.7.4.4: the local speed 60 observed at 11:00 is
/// in effect at 12:00 alone, and a source's speed 40 observed at 11:30
/// supersedes it, so the Vehicle is kept without the source's answer and
/// dropped with it.
#[tokio::test]
async fn a_local_instance_in_effect_a_probe_answer_supersedes_is_not_retained() {
    let broker = || {
        StubBroker::new([])
            .judging(uri(VEHICLE), LeafTruths::none(1))
            .holding_in_effect(uri(VEHICLE), vec![speed(60.0, "2020-08-01T11:00:00Z")])
    };
    let local = local_in_effect(broker(), ABOVE_50).await;
    let window = vehicle_part(json!({"brand": [{"type": "Property", "value": "Mercedes", "observedAt": "2020-08-01T12:10:00Z"}]})).await;
    let probe = vehicle_part(json!({"speed": [{"type": "Property", "value": 40, "observedAt": "2020-08-01T11:30:00Z"}]})).await;
    pretty_assertions::assert_eq!(retained(broker(), ABOVE_50, &[&window], &[], &local).await, vec![uri(VEHICLE)]);
    pretty_assertions::assert_eq!(retained(broker(), ABOVE_50, &[&window], &[&probe], &local).await, Vec::<Identifier>::new());
}

/// Clause 5.7.4.4: the leaves of a conjunction may each hold in a different
/// kind of part, one inside the window here and one in effect elsewhere.
#[tokio::test]
async fn each_leaf_may_hold_in_another_kind_of_part() {
    let q = "\"https://example.com/speed\">50;\"https://example.com/brand\"==\"Mercedes\"";
    let broker = || StubBroker::new([]).judging(uri(VEHICLE), LeafTruths::from_vec(vec![false, true]));
    let probe = vehicle_part(json!({"speed": [{"type": "Property", "value": 60, "observedAt": "2020-08-01T11:00:00Z"}]})).await;
    let nothing = LocalInEffect::default();
    pretty_assertions::assert_eq!(retained(broker(), q, &[], &[&probe], &nothing).await, vec![uri(VEHICLE)]);
    let slow = vehicle_part(json!({"speed": [{"type": "Property", "value": 40, "observedAt": "2020-08-01T11:00:00Z"}]})).await;
    pretty_assertions::assert_eq!(retained(broker(), q, &[], &[&slow], &nothing).await, Vec::<Identifier>::new());
}

const FREE_PARKING: &str = "\"https://example.com/parkedIn\"{\"https://example.com/free\"}>0";

/// Clause 4.9 under `temporal_linked_retrieval`: a source's Relationship
/// reaches the entity this broker holds, judged on its instances inside the
/// window.
#[tokio::test]
async fn a_remote_relationship_reaches_a_local_target_over_the_window() {
    let window = vehicle_part(json!({
        "https://example.com/parkedIn": [{"type": "Relationship", "object": PARKING, "observedAt": "2020-08-01T12:10:00Z"}]
    }))
    .await;
    let nothing = LocalInEffect::default();
    let free_spots = StubBroker::new([]).paging(uri(PARKING), vec![free(3.0, "2020-08-01T12:20:00Z")]);
    pretty_assertions::assert_eq!(retained(free_spots, FREE_PARKING, &[&window], &[], &nothing).await, vec![uri(VEHICLE)]);
    let full = StubBroker::new([]).paging(uri(PARKING), vec![free(0.0, "2020-08-01T12:20:00Z")]);
    pretty_assertions::assert_eq!(retained(full, FREE_PARKING, &[&window], &[], &nothing).await, Vec::<Identifier>::new());
}

/// The Relationship this broker holds inside the window reaches its target in
/// memory too, beside a source's part of the same Vehicle.
#[tokio::test]
async fn a_local_relationship_reaches_its_target_in_memory() {
    let broker = StubBroker::new([])
        .relating(uri(VEHICLE), vec![parked_in("2020-08-01T12:10:00Z")])
        .paging(uri(PARKING), vec![free(3.0, "2020-08-01T12:20:00Z")]);
    let window = vehicle_part(json!({"brand": [{"type": "Property", "value": "Mercedes", "observedAt": "2020-08-01T12:10:00Z"}]})).await;
    let nothing = LocalInEffect::default();
    pretty_assertions::assert_eq!(retained(broker, FREE_PARKING, &[&window], &[], &nothing).await, vec![uri(VEHICLE)]);
}

/// Clause 5.7.4.4: without a value, geo or scope filter S7 is S4, so every
/// contributed entity is kept.
#[tokio::test]
async fn a_query_without_filters_keeps_every_contributed_entity() {
    let window = vehicle_part(json!({"brand": [{"type": "Property", "value": "Mercedes", "observedAt": "2020-08-01T12:10:00Z"}]})).await;
    let service = service(StubBroker::new([]));
    let params = TemporalQueryParameters {
        q: None,
        ..split_query(ABOVE_50)
    };
    let parts = AnsweredParts {
        in_window: vec![&window],
        in_effect: Vec::new(),
    };
    let settlement = service
        .settle_split_filters(
            &live_target(),
            &params,
            QueryReading {
                context: &ContextSpec::none(),
                language_targeting: LanguageTargeting::Specification,
            },
            &parts,
            &LocalInEffect::default(),
        )
        .await
        .expect("the settlement runs");
    pretty_assertions::assert_eq!(settlement.retained.into_iter().collect::<Vec<_>>(), vec![uri(VEHICLE)]);
}
