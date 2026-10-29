use crate::{
    answered_fixture::answered,
    error::TemporalError,
    local_in_effect::LocalInEffect,
    query_reading::QueryReading,
    remote_linked::{RemoteTemporalLinkedSource, TemporalLinkedWindow},
    split_settlement::{AnsweredParts, SplitSettlement},
    stub_broker::{StubBroker, live_target, service, service_over, uri},
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_linked_retrieval::source::{RemoteReach, RemoteResolution};
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
    temporal_history::TemporalHistory,
};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_query::{filter::language_targeting::LanguageTargeting, split_filter::LeafTruths};
use andromeda_ngsi_ld_representation::linked_resolution_failure::LinkedResolutionFailureKind;
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    num::NonZeroU32,
    sync::{Arc, Mutex},
};

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

const OPERATED_BY: &str = "\"https://example.com/parkedIn\"{\"https://example.com/operator\"{\"https://example.com/employees\"}}>10";

/// The remainder of the system as the entities it holds, a request it refuses,
/// and the attributes each request asked for.
#[derive(Debug)]
struct OtherHolders {
    /// What the sources answer for each entity: one history per source.
    held: HashMap<Identifier, Vec<TemporalHistory>>,
    /// Whether the remainder of the system cannot be consulted at all.
    unreachable: bool,
    /// The entities and attributes each request named, in order.
    asked: Mutex<Vec<(Identifier, Option<Vec<AttributeName>>)>>,
}

impl OtherHolders {
    fn new(held: impl IntoIterator<Item = (Identifier, Vec<TemporalHistory>)>) -> Arc<OtherHolders> {
        Arc::new(OtherHolders {
            held: held.into_iter().collect(),
            unreachable: false,
            asked: Mutex::new(Vec::new()),
        })
    }

    fn unreachable() -> Arc<OtherHolders> {
        Arc::new(OtherHolders {
            held: HashMap::new(),
            unreachable: true,
            asked: Mutex::new(Vec::new()),
        })
    }

    fn asked(&self) -> Vec<(Identifier, Option<Vec<AttributeName>>)> {
        self.asked.lock().expect("the lock is never poisoned").clone() // clone: the lock is released before the test compares
    }
}

#[async_trait]
impl RemoteTemporalLinkedSource for OtherHolders {
    async fn remote_reach(&self, _id: &Identifier, _context: &ContextSpec) -> RemoteReach {
        RemoteReach::SingleEntity
    }

    async fn fetch_linked_temporal(
        &self,
        id: &Identifier,
        _remaining_depth: NonZeroU32,
        _visited: &[Identifier],
        window: &TemporalLinkedWindow,
        _context: &ContextSpec,
    ) -> Result<RemoteResolution<TemporalHistory>, TemporalError> {
        self.asked.lock().expect("the lock is never poisoned").push((id.clone(), window.attrs.clone())); // clone: the recording owns what it reports
        if self.unreachable {
            return Err(TemporalError::from(PersistenceFailure::Unavailable));
        }
        Ok(RemoteResolution {
            entities: self.held.get(id).cloned().unwrap_or_default(), // clone: the source hands back owned values
            failures: Vec::new(),
        })
    }
}

/// What a source holds of `id`, as it answers for it.
async fn held_by_source(id: &str, members: Value) -> (Identifier, Vec<TemporalHistory>) {
    let mut answer = json!({"id": id, "type": "Parking"});
    if let (Value::Object(entity), Value::Object(members)) = (&mut answer, members) {
        entity.extend(members);
    }
    (uri(id), vec![TemporalHistory::Instances(answered(&answer).await)])
}

/// The settlement of `q` over the Vehicle part `window`, this broker holding
/// `broker` and the remainder of the system `others`.
async fn settled_beside(broker: StubBroker, others: &Arc<OtherHolders>, q: &str, window: &TemporalDocument) -> SplitSettlement {
    let service = service_over(Arc::new(broker), Some(Arc::clone(others) as Arc<dyn RemoteTemporalLinkedSource>)); // clone: the test keeps its handle on the recording
    let parts = AnsweredParts {
        in_window: vec![window],
        in_effect: Vec::new(),
    };
    service
        .settle_split_filters(
            &live_target(),
            &split_query(q),
            QueryReading {
                context: &ContextSpec::none(),
                language_targeting: LanguageTargeting::Specification,
            },
            &parts,
            &LocalInEffect::default(),
        )
        .await
        .expect("the settlement runs")
}

async fn parked_vehicle() -> TemporalDocument {
    vehicle_part(json!({
        "https://example.com/parkedIn": [{"type": "Relationship", "object": PARKING, "observedAt": "2020-08-01T12:10:00Z"}]
    }))
    .await
}

/// Clause 5.7.4.4: a hop target whose first half this broker holds and whose
/// second half a source holds is judged on both, so the later source instance
/// `free>0` holds where the local `free=0` alone does not.
#[tokio::test]
async fn a_target_split_between_this_broker_and_a_source_is_judged_on_both_halves() {
    let window = parked_vehicle().await;
    let local = || StubBroker::new([]).paging(uri(PARKING), vec![free(0.0, "2020-08-01T12:20:00Z")]);
    let source = OtherHolders::new([held_by_source(
        PARKING,
        json!({"https://example.com/free": [{"type": "Property", "value": 3, "observedAt": "2020-08-01T12:40:00Z"}]}),
    )
    .await]);
    pretty_assertions::assert_eq!(
        settled_beside(local(), &source, FREE_PARKING, &window)
            .await
            .retained
            .into_iter()
            .collect::<Vec<_>>(),
        vec![uri(VEHICLE)]
    );
    let silent = OtherHolders::new([]);
    pretty_assertions::assert_eq!(settled_beside(local(), &silent, FREE_PARKING, &window).await.retained.len(), 0);
}

/// A target no registration of this broker records is held by two sources,
/// each with one half of it; the halves are merged before the hop is judged.
#[tokio::test]
async fn a_target_held_only_by_two_sources_is_judged_on_the_union() {
    let window = parked_vehicle().await;
    let (id, mut first) = held_by_source(
        PARKING,
        json!({"https://example.com/free": [{"type": "Property", "value": 0, "observedAt": "2020-08-01T12:20:00Z"}]}),
    )
    .await;
    let (_, second) = held_by_source(
        PARKING,
        json!({"https://example.com/free": [{"type": "Property", "value": 3, "observedAt": "2020-08-01T12:40:00Z"}]}),
    )
    .await;
    first.extend(second);
    let sources = OtherHolders::new([(id, first)]);
    let settlement = settled_beside(StubBroker::new([]), &sources, FREE_PARKING, &window).await;
    pretty_assertions::assert_eq!(settlement.retained.into_iter().collect::<Vec<_>>(), vec![uri(VEHICLE)]);
    pretty_assertions::assert_eq!(settlement.incomplete.len(), 0);
}

/// Clause 4.5.23: a source that cannot be consulted leaves its part out, the
/// part this broker holds still decides the hop, and the failure is reported.
#[tokio::test]
async fn a_failing_source_leaves_its_part_out_and_is_reported() {
    let window = parked_vehicle().await;
    let local = StubBroker::new([]).paging(uri(PARKING), vec![free(3.0, "2020-08-01T12:20:00Z")]);
    let settlement = settled_beside(local, &OtherHolders::unreachable(), FREE_PARKING, &window).await;
    pretty_assertions::assert_eq!(settlement.retained.into_iter().collect::<Vec<_>>(), vec![uri(VEHICLE)]);
    pretty_assertions::assert_eq!(
        settlement
            .incomplete
            .iter()
            .map(|failure| (failure.id.clone(), failure.kind))
            .collect::<Vec<_>>(), // clone: the expectation owns the identifier
        vec![(uri(PARKING), LinkedResolutionFailureKind::Unreachable)]
    );
}

/// Every depth of a two-hop term reads the union of its holders: the Parking
/// is local, the Company it names a source's, and each is asked only for the
/// attribute the hops read of it.
#[tokio::test]
async fn a_two_hop_term_reads_each_depth_from_every_holder_for_the_attributes_it_needs() {
    let window = parked_vehicle().await;
    let local = StubBroker::new([]).paging(
        uri(PARKING),
        vec![row(
            "https://example.com/operator",
            AttributeType::Relationship,
            TemporalValue::Uri(uri("urn:ngsi-ld:Company:1").as_uri().clone()), // clone: the row owns the object it names
            "2020-08-01T12:20:00Z",
        )],
    );
    let company = held_by_source(
        "urn:ngsi-ld:Company:1",
        json!({"https://example.com/employees": [{"type": "Property", "value": 40, "observedAt": "2020-08-01T12:30:00Z"}]}),
    )
    .await;
    let source = OtherHolders::new([company]);
    let settlement = settled_beside(local, &source, OPERATED_BY, &window).await;
    pretty_assertions::assert_eq!(settlement.retained.into_iter().collect::<Vec<_>>(), vec![uri(VEHICLE)]);
    let name = |iri: &str| iri.parse::<AttributeName>().expect("a valid attribute IRI");
    pretty_assertions::assert_eq!(
        source.asked(),
        vec![
            (uri(PARKING), Some(vec![name("https://example.com/operator")])),
            (uri("urn:ngsi-ld:Company:1"), Some(vec![name("https://example.com/employees")])),
        ]
    );
}
