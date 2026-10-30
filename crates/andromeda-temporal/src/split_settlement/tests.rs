use crate::{
    answered_fixture::answered,
    error::TemporalError,
    local_in_effect::LocalInEffect,
    query_reading::QueryReading,
    remote_linked::{HeldPart, RemoteTemporalLinkedSource, TemporalLinkedWindow},
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
use andromeda_ngsi_ld_query::{dataset::DatasetSelector, filter::language_targeting::LanguageTargeting, split_filter::LeafTruths};
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

/// What a probe for the instances before an instant names: the entity, the
/// Temporal Property, the exclusive bound, the one dataset and the one
/// registration asked, when the probe names them.
type ProbeKey = (Identifier, TimeProp, DateTime<Utc>, Option<DatasetSelector>, Option<Identifier>);

/// One holder's scripted answer to a probe of an entity before an instant.
#[derive(Debug)]
struct Probed {
    /// The entity, the Temporal Property, the bound and the dataset the probe
    /// names.
    asks: (Identifier, TimeProp, DateTime<Utc>, Option<DatasetSelector>),
    /// The registration of the holder answering.
    registration: Identifier,
    /// What it answers.
    history: TemporalHistory,
}

/// The remainder of the system as the entities it holds, a request it refuses,
/// and the attributes each request asked for.
#[derive(Debug)]
struct OtherHolders {
    /// What the sources answer for each entity: one history per source.
    held: HashMap<Identifier, Vec<TemporalHistory>>,
    /// What each holder answers each probe with.
    probes: Vec<Probed>,
    /// Whether the remainder of the system cannot be consulted at all.
    unreachable: bool,
    /// The entities and attributes each request named, in order.
    asked: Mutex<Vec<(Identifier, Option<Vec<AttributeName>>)>>,
    /// Every probe sent, in order.
    probed: Mutex<Vec<ProbeKey>>,
}

impl OtherHolders {
    fn new(held: impl IntoIterator<Item = (Identifier, Vec<TemporalHistory>)>) -> Arc<OtherHolders> {
        OtherHolders::probing(held, Vec::new())
    }

    fn probing(held: impl IntoIterator<Item = (Identifier, Vec<TemporalHistory>)>, probes: Vec<Probed>) -> Arc<OtherHolders> {
        Arc::new(OtherHolders {
            held: held.into_iter().collect(),
            probes,
            unreachable: false,
            asked: Mutex::new(Vec::new()),
            probed: Mutex::new(Vec::new()),
        })
    }

    fn unreachable() -> Arc<OtherHolders> {
        Arc::new(OtherHolders {
            held: HashMap::new(),
            probes: Vec::new(),
            unreachable: true,
            asked: Mutex::new(Vec::new()),
            probed: Mutex::new(Vec::new()),
        })
    }

    fn probes_sent(&self) -> Vec<ProbeKey> {
        self.probed.lock().expect("the lock is never poisoned").clone() // clone: the lock is released before the test compares
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

    async fn fetch_held_parts(
        &self,
        id: &Identifier,
        params: &TemporalQueryParameters,
        only: Option<&Identifier>,
        _context: &ContextSpec,
    ) -> Result<RemoteResolution<HeldPart>, TemporalError> {
        let timeproperty = params.timeproperty_or_default();
        let before = params.time_at.expect("a probe names its bound");
        let dataset = params.dataset_id.as_ref().and_then(|datasets| datasets.first()).cloned(); // clone: the recording owns what it reports
        // clone: the recording owns what it reports
        self.probed
            .lock()
            .expect("the lock is never poisoned")
            .push((id.clone(), timeproperty, before, dataset.clone(), only.cloned()));
        if self.unreachable {
            return Err(TemporalError::from(PersistenceFailure::Unavailable));
        }
        let asks = (id.clone(), timeproperty, before, dataset); // clone: the key owns the identifier it compares
        Ok(RemoteResolution {
            entities: self
                .probes
                .iter()
                .filter(|probe| probe.asks == asks && only.is_none_or(|only| *only == probe.registration))
                .map(|probe| HeldPart {
                    registration: Some(probe.registration.clone()), // clone: the source hands back owned values
                    history: probe.history.clone(),
                })
                .collect(),
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

const FREE: &str = "https://example.com/free";
const NOON: &str = "2020-08-01T12:00:00Z";

/// What a holder answers of the Parking, holding `members`.
async fn parking_history(members: Value) -> TemporalHistory {
    let (_, mut histories) = held_by_source(PARKING, members).await;
    histories.remove(0)
}

/// The Parking holding `free` spots, observed at `observed`, in `dataset`
/// when one is named.
fn free_spots(free: u32, observed: &str, dataset: Option<&str>) -> Value {
    let mut instance = json!({"type": "Property", "value": free, "observedAt": observed});
    if let (Some(dataset), Value::Object(instance)) = (dataset, &mut instance) {
        instance.insert("datasetId".to_owned(), json!(dataset));
    }
    json!({FREE: [instance]})
}

/// The answer `registration` gives to the probe of the Parking before
/// `before` on `timeproperty`, in `dataset` when one is named.
async fn probed(registration: &str, timeproperty: TimeProp, before: &str, dataset: Option<DatasetSelector>, members: Value) -> Probed {
    Probed {
        asks: (uri(PARKING), timeproperty, at(before), dataset),
        registration: uri(registration),
        history: parking_history(members).await,
    }
}

/// Whether the parked Vehicle is kept under `parkedIn{free}>0`, this broker
/// holding `broker` and the remainder of the system answering `probes`.
async fn parked_vehicle_kept(broker: StubBroker, probes: Vec<Probed>) -> bool {
    let window = parked_vehicle().await;
    let holders = OtherHolders::probing([], probes);
    !settled_beside(broker, &holders, FREE_PARKING, &window).await.retained.is_empty()
}

/// Clause 4.11: the Parking was last written at 11:00 and holds nothing
/// inside the window, so the hop reads the value in effect at 12:00.
#[tokio::test]
async fn a_hop_target_written_only_before_the_window_is_judged_on_its_value_in_effect() {
    for (free, kept) in [(3, true), (0, false)] {
        let probes = vec![
            probed(
                "urn:ngsi-ld:csr:1",
                TimeProp::Observed,
                NOON,
                None,
                free_spots(free, "2020-08-01T11:00:00Z", None),
            )
            .await,
        ];
        pretty_assertions::assert_eq!(parked_vehicle_kept(StubBroker::new([]), probes).await, kept, "{free} free spots");
    }
}

/// This broker's own history answers for a target it records alone.
#[tokio::test]
async fn a_hop_target_this_broker_wrote_only_before_the_window_is_judged_on_its_value_in_effect() {
    for (spots, kept) in [(3.0, true), (0.0, false)] {
        let broker = StubBroker::new([(uri(PARKING), Vec::new())]).holding_in_effect(uri(PARKING), vec![free(spots, "2020-08-01T11:00:00Z")]);
        let window = parked_vehicle().await;
        let settlement = settled_beside(broker, &OtherHolders::new([]), FREE_PARKING, &window).await;
        pretty_assertions::assert_eq!(!settlement.retained.is_empty(), kept, "{spots} free spots");
    }
}

/// Clause 5.7.4.4: the instance in effect is the latest across every holder,
/// so a later value another holder keeps supersedes the one this broker or a
/// first source holds, and an older one does not.
#[tokio::test]
async fn a_later_value_on_another_holder_supersedes_the_value_in_effect() {
    let local = || StubBroker::new([(uri(PARKING), Vec::new())]).holding_in_effect(uri(PARKING), vec![free(3.0, "2020-08-01T11:00:00Z")]);
    let other = |observed: &'static str| async move { vec![probed("urn:ngsi-ld:csr:2", TimeProp::Observed, NOON, None, free_spots(0, observed, None)).await] };
    assert!(!parked_vehicle_kept(local(), other("2020-08-01T11:30:00Z").await).await);
    assert!(parked_vehicle_kept(local(), other("2020-08-01T10:30:00Z").await).await);
    let first = probed("urn:ngsi-ld:csr:1", TimeProp::Observed, NOON, None, free_spots(3, "2020-08-01T11:00:00Z", None)).await;
    let mut probes = other("2020-08-01T11:30:00Z").await;
    probes.push(first);
    assert!(!parked_vehicle_kept(StubBroker::new([]), probes).await);
}

/// The undated deletion of `free`, deleted at `deleted_at`.
fn free_deleted(deleted_at: &str) -> Value {
    json!({FREE: [{"type": "Property", "value": "urn:ngsi-ld:null", "deletedAt": deleted_at}]})
}

/// Clause 4.5.7: a deletion placed after the value in effect leaves nothing
/// in effect, whichever holder recorded it, so the hop is never satisfied.
#[tokio::test]
async fn a_hop_target_deleted_before_the_window_never_satisfies_the_hop() {
    let value = || probed("urn:ngsi-ld:csr:1", TimeProp::Observed, NOON, None, free_spots(3, "2020-08-01T11:00:00Z", None));
    let deletion = probed("urn:ngsi-ld:csr:2", TimeProp::Deleted, NOON, None, free_deleted("2020-08-01T11:30:00Z")).await;
    assert!(!parked_vehicle_kept(StubBroker::new([]), vec![value().await, deletion]).await);
    let older = probed("urn:ngsi-ld:csr:2", TimeProp::Deleted, NOON, None, free_deleted("2020-08-01T10:30:00Z")).await;
    assert!(parked_vehicle_kept(StubBroker::new([]), vec![value().await, older]).await);
}

/// Clauses 4.5.7 and 4.11: the deletion with the latest `deletedAt` is dated
/// at 10:00, below the value in effect, while an undated deletion with an
/// older `deletedAt` is placed at 11:20, so the source that answered is asked
/// again, alone, before the `deletedAt` it returned, and the one it left out
/// ends the stream. Without it the value observed at 11:00 is in effect.
#[tokio::test]
async fn an_older_deletion_a_holder_left_out_is_asked_for_and_ends_the_stream() {
    let source = "urn:ngsi-ld:csr:1";
    let first_answers = || async {
        let dated =
            json!({FREE: [{"type": "Property", "value": "urn:ngsi-ld:null", "observedAt": "2020-08-01T10:00:00Z", "deletedAt": "2020-08-01T11:50:00Z"}]});
        vec![
            probed(source, TimeProp::Observed, NOON, None, free_spots(3, "2020-08-01T11:00:00Z", None)).await,
            probed(source, TimeProp::Deleted, NOON, None, dated).await,
        ]
    };
    assert!(parked_vehicle_kept(StubBroker::new([]), first_answers().await).await);
    let mut probes = first_answers().await;
    probes.push(
        probed(
            source,
            TimeProp::Deleted,
            "2020-08-01T11:50:00Z",
            Some(DatasetSelector::Default),
            free_deleted("2020-08-01T11:20:00Z"),
        )
        .await,
    );
    let window = parked_vehicle().await;
    let holders = OtherHolders::probing([], probes);
    assert!(settled_beside(StubBroker::new([]), &holders, FREE_PARKING, &window).await.retained.is_empty());
    assert!(
        holders.probes_sent().contains(&(
            uri(PARKING),
            TimeProp::Deleted,
            at("2020-08-01T11:50:00Z"),
            Some(DatasetSelector::Default),
            Some(uri(source))
        )),
        "{:?}",
        holders.probes_sent()
    );
}

/// Table 6.18.3.2-1 with clause 4.5.5: the first source applies `lastN` per
/// Attribute and answers its latest dataset `d2` alone, while the second holds
/// `d1`, so the first is asked about `d1` on its own, and its later `d1`
/// value supersedes the second source's, which is in effect without it.
#[tokio::test]
async fn a_dataset_a_holder_left_out_of_its_answer_is_asked_about_on_its_own() {
    const D1: &str = "urn:ngsi-ld:dataset:d1";
    const D2: &str = "urn:ngsi-ld:dataset:d2";
    let d1 = DatasetSelector::Id(D1.parse().expect("a valid identifier"));
    let first_answers = || async {
        vec![
            probed(
                "urn:ngsi-ld:csr:1",
                TimeProp::Observed,
                NOON,
                None,
                free_spots(0, "2020-08-01T11:40:00Z", Some(D2)),
            )
            .await,
            probed(
                "urn:ngsi-ld:csr:2",
                TimeProp::Observed,
                NOON,
                None,
                free_spots(3, "2020-08-01T11:00:00Z", Some(D1)),
            )
            .await,
        ]
    };
    assert!(parked_vehicle_kept(StubBroker::new([]), first_answers().await).await);
    let mut probes = first_answers().await;
    probes.push(
        probed(
            "urn:ngsi-ld:csr:1",
            TimeProp::Observed,
            NOON,
            Some(d1.clone()),
            free_spots(0, "2020-08-01T11:30:00Z", Some(D1)),
        )
        .await,
    ); // clone: the expectation below names the dataset too
    let window = parked_vehicle().await;
    let holders = OtherHolders::probing([], probes);
    assert!(settled_beside(StubBroker::new([]), &holders, FREE_PARKING, &window).await.retained.is_empty());
    assert!(
        holders.probes_sent().contains(&(uri(PARKING), TimeProp::Observed, at(NOON), Some(d1), None)),
        "{:?}",
        holders.probes_sent()
    );
}

/// Clause 4.9: hops nest, so a Relationship the Parking holds in effect when
/// the window opens names the Company the second hop reads.
#[tokio::test]
async fn a_relationship_in_effect_when_the_window_opens_reaches_the_next_depth() {
    let window = parked_vehicle().await;
    let local = StubBroker::new([(uri(PARKING), Vec::new())]).holding_in_effect(
        uri(PARKING),
        vec![row(
            "https://example.com/operator",
            AttributeType::Relationship,
            TemporalValue::Uri(uri("urn:ngsi-ld:Company:1").as_uri().clone()), // clone: the row owns the object it names
            "2020-08-01T11:00:00Z",
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
}

/// Each holder is asked only for the attributes the hop reads of the target,
/// on the window's Temporal Property and on `deletedAt`, before the start.
#[tokio::test]
async fn the_holders_are_asked_for_the_latest_instance_and_deletion_before_the_start() {
    let window = parked_vehicle().await;
    let holders = OtherHolders::probing([], Vec::new());
    let _ = settled_beside(StubBroker::new([]), &holders, FREE_PARKING, &window).await;
    pretty_assertions::assert_eq!(
        holders.probes_sent(),
        vec![
            (uri(PARKING), TimeProp::Observed, at(NOON), None, None),
            (uri(PARKING), TimeProp::Deleted, at(NOON), None, None),
        ]
    );
}
