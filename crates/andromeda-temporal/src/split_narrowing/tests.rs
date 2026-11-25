use crate::{
    answered_fixture::answered,
    local_in_effect::LocalInEffect,
    query_reading::QueryReading,
    split_settlement::AnsweredParts,
    stub_broker::{StubBroker, admitted, live_target, service, uri},
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{time::date_time::NgsiLdDateTime, time_prop::TimeProp};
use andromeda_ngsi_ld_model::temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory};
use andromeda_ngsi_ld_query::{
    filter::language_targeting::LanguageTargeting,
    temporal::contributed_entities::{ContributedEntities, ContributedRendering},
};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

const VEHICLE: &str = "urn:ngsi-ld:Vehicle:1";
const SPEED: &str = "https://example.com/speed";
const FUEL_LEVEL: &str = "https://example.com/fuelLevel";
const BRAND: &str = "https://example.com/brand";
const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

/// The window every case reads: between 12:00 and 13:00 on `observedAt`.
const WINDOW: &str = "type=https%3A%2F%2Fexample.com%2FVehicle&timerel=between&timeAt=2020-08-01T12:00:00Z&endTimeAt=2020-08-01T13:00:00Z";

fn at(raw: &str) -> DateTime<Utc> {
    raw.parse::<NgsiLdDateTime>().expect("a valid instant").get()
}

/// The split query over [`WINDOW`] that `filters` adds to.
fn query(filters: &str) -> TemporalQueryParameters {
    serde_urlencoded::from_str(&format!("{WINDOW}&{filters}")).expect("a valid temporal query")
}

/// Vehicle 1 holding `members`, as a source answered it.
async fn vehicle(members: Value) -> TemporalDocument {
    let mut answer = json!({"id": VEHICLE, "type": "Vehicle"});
    if let (Value::Object(entity), Value::Object(members)) = (&mut answer, members) {
        entity.extend(members);
    }
    answered(&answer).await
}

/// One `Property` instance holding `value`, observed at `observed` minutes
/// past noon.
fn observed(value: u32, minute: u32) -> Value {
    json!({"type": "Property", "value": value, "observedAt": format!("2020-08-01T12:{minute:02}:00Z")})
}

/// One `location` instance at `(longitude, latitude)`, observed at `minute`
/// minutes past noon.
fn located(longitude: f64, latitude: f64, minute: u32) -> Value {
    json!({
        "type": "GeoProperty",
        "value": {"type": "Point", "coordinates": [longitude, latitude]},
        "observedAt": format!("2020-08-01T12:{minute:02}:00Z")
    })
}

/// What the split query `params` answers of `part`, held by one source and
/// no part of it here, the settlement judging the entity over its parts and
/// the narrowing keeping the instances of the kept entity that meet the
/// restrictions.
async fn narrowed(params: TemporalQueryParameters, part: TemporalDocument) -> Vec<TemporalHistory> {
    let parts = AnsweredParts {
        in_window: vec![&part],
        in_effect: Vec::new(),
    };
    let settlement = service(StubBroker::new([]))
        .settle_split_filters(
            &live_target(),
            &admitted(params),
            QueryReading {
                context: &ContextSpec::none(),
                language_targeting: LanguageTargeting::Specification,
            },
            &parts,
            &LocalInEffect::default(),
        )
        .await
        .expect("the settlement runs");
    let contributed = ContributedEntities::new([uri(VEHICLE)], ContributedRendering::WholeWindow);
    assert!(settlement.keeps(&contributed, &uri(VEHICLE)), "the filters select the entity");
    let mut entities = vec![TemporalHistory::Instances(part.clone())];
    if let Some(narrowing) = &settlement.narrowing {
        narrowing.narrow(&mut entities, &contributed);
    }
    entities
}

/// The instants of the instances of the attribute `name` each entity of
/// `entities` keeps.
fn kept(entities: &[TemporalHistory], name: &str) -> Vec<DateTime<Utc>> {
    entities
        .iter()
        .filter_map(|entity| match entity {
            TemporalHistory::Instances(document) => Some(document),
            TemporalHistory::Aggregates(_) => None,
        })
        .flat_map(TemporalDocument::instances)
        .filter(|(held, _)| held.as_str() == name)
        .filter_map(|(_, instance)| instance.instant_on(TimeProp::Observed))
        .collect()
}

/// Clause 5.7.4.4 keeps the instances "which meet the temporal, query and
/// geoquery restrictions", so of speeds 120, 80, 100 and 95 `speed>90` keeps
/// the three above 90.
#[tokio::test]
async fn a_value_filter_keeps_the_instances_of_its_attribute_that_meet_it() {
    let part = vehicle(json!({"speed": [observed(120, 1), observed(80, 2), observed(100, 3), observed(95, 4)]})).await;
    let entities = narrowed(query("q=%22https://example.com/speed%22%3E90"), part).await;
    pretty_assertions::assert_eq!(
        kept(&entities, SPEED),
        vec![at("2020-08-01T12:01:00Z"), at("2020-08-01T12:03:00Z"), at("2020-08-01T12:04:00Z")]
    );
}

/// `speed>90|fuelLevel<35` keeps speed 95, which meets it, and fuelLevel 40,
/// since the filter holds with `fuelLevel` reduced to it through speed 95;
/// speed 80 meets neither term.
#[tokio::test]
async fn a_disjunction_keeps_an_instance_the_other_attribute_satisfies() {
    let part = vehicle(json!({"speed": [observed(95, 1), observed(80, 2)], "fuelLevel": [observed(40, 1)]})).await;
    let entities = narrowed(query("q=%22https://example.com/speed%22%3E90%7C%22https://example.com/fuelLevel%22%3C35"), part).await;
    pretty_assertions::assert_eq!(kept(&entities, SPEED), vec![at("2020-08-01T12:01:00Z")]);
    pretty_assertions::assert_eq!(kept(&entities, FUEL_LEVEL), vec![at("2020-08-01T12:01:00Z")]);
}

/// `speed<50;speed>90` selects an Entity whose speeds 30 and 100 each meet a
/// term, but no single instance meets both, so no `speed` instance is kept.
/// The Entity keeps its other Attributes, and one left with no instance is no
/// part of the answer, as the local read leaves out an Entity it reads no
/// instance of.
#[tokio::test]
async fn a_conjunction_no_single_instance_meets_keeps_no_instance_of_it() {
    let restricted = "q=%22https://example.com/speed%22%3C50%3B%22https://example.com/speed%22%3E90";
    let with_brand = vehicle(json!({
        "speed": [observed(30, 1), observed(100, 2)],
        "brand": [{"type": "Property", "value": "Audi", "observedAt": "2020-08-01T12:01:00Z"}]
    }))
    .await;
    let entities = narrowed(query(restricted), with_brand).await;
    pretty_assertions::assert_eq!(kept(&entities, SPEED), Vec::<DateTime<Utc>>::new());
    pretty_assertions::assert_eq!(kept(&entities, BRAND), vec![at("2020-08-01T12:01:00Z")]);
    let speed_only = vehicle(json!({"speed": [observed(30, 1), observed(100, 2)]})).await;
    assert!(narrowed(query(restricted), speed_only).await.is_empty());
}

/// Clause 5.7.4.4 keeps the `GeoProperty` instances that meet the geo-query,
/// so of a `location` 500 m from the point and one 900 km away `near` within
/// 2000 m keeps the first.
#[tokio::test]
async fn a_geo_query_keeps_the_instances_of_its_geo_property_that_meet_it() {
    let part = vehicle(json!({"location": [located(4.5, 52.5045, 1), located(4.5, 60.6, 2)]})).await;
    let entities = narrowed(query("georel=near%3BmaxDistance%3D%3D2000&geometry=Point&coordinates=%5B4.5%2C52.5%5D"), part).await;
    pretty_assertions::assert_eq!(kept(&entities, LOCATION), vec![at("2020-08-01T12:01:00Z")]);
}
