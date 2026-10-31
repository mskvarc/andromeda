//! The world the linked temporal retrieval tests share: Parkings and Vehicles on two brokers, the registration of the
//! peer, and the between query that judges a hop target by the value in effect when the window opens (clause 4.11).

use crate::{
    body_json::{json_count, json_equals},
    broker::BrokerClient,
    namespace::Namespace,
    outcome::Upserted,
    problem::answered,
    query_join::with_join,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryTemporalEvolution},
    model::{EntityId, Input, Lossless, NgsiLdMediaType, Property, Relationship, evolution::TemporalEntity},
    query::{EntitySplitting, Join, builders::SelectorTypeBuilder, q, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationId, RegistrationInfo},
};
use url::Url;

/// The temporal API operations every registration of the hop tests offers.
const OPERATIONS: [DistributedOperation; 2] = [DistributedOperation::QueryTemporal, DistributedOperation::RetrieveTemporal];

/// Writes one `free` instance of Parking `number` to `client` through Create Or Update Temporal Evolution, observed at
/// `observed` and in the dataset named `dataset` when it is given.
///
/// # Errors
///
/// Fails when the instant does not parse or the broker does not answer 201.
pub async fn write_parking(client: &BrokerClient, namespace: &Namespace, number: u32, free: i32, observed: &str, dataset: Option<&str>) -> Result<()> {
    let builder = TemporalEntity::builder()
        .id(namespace.urn("Parking", number)?)
        .entity_type(namespace.entity_type("Parking")?);
    let parking = match dataset {
        Some(name) => builder
            .property(
                "free",
                Property::builder(free)
                    .dataset_id(namespace.dataset_id("dataset", name)?)
                    .observed_at(observed.parse()?),
            )
            .build()?,
        None => builder.property("free", Property::builder(free).observed_at(observed.parse()?)).build()?,
    };
    client.temporal().create_or_update(&parking).send().await?.created()?;
    Ok(())
}

/// Writes Vehicles 1 and 2: `speed` 60 at 12:10 on `broker_b`, and on `broker_e` the `parkedIn` relationship of
/// Vehicle `n` to Parking `n` observed at 12:10.
///
/// # Errors
///
/// Fails when a write is not answered 201.
pub async fn write_vehicles(broker_b: &BrokerClient, broker_e: &BrokerClient, namespace: &Namespace) -> Result<()> {
    let vehicle_type = namespace.entity_type("Vehicle")?;
    for number in [1_u32, 2] {
        let speed = TemporalEntity::builder()
            .id(namespace.urn("Vehicle", number)?)
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(60).observed_at("2020-08-01T12:10:00Z".parse()?))
            .build()?;
        broker_b.temporal().create_or_update(&speed).send().await?.created()?;
    }
    for number in [1_u32, 2] {
        let parked = TemporalEntity::builder()
            .id(namespace.urn("Vehicle", number)?)
            .entity_type(vehicle_type.clone())
            .relationship(
                "parkedIn",
                Relationship::builder(namespace.urn("Parking", number)?).observed_at("2020-08-01T12:10:00Z".parse()?),
            )
            .build()?;
        broker_e.temporal().create_or_update(&parked).send().await?.created()?;
    }
    Ok(())
}

/// Registers the peer broker B on `broker_e` for the Vehicles and Parkings of the test, as registration 1.
///
/// # Errors
///
/// Fails when the registration cannot be built or the broker refuses it.
pub async fn register_peer(broker_e: &BrokerClient, namespace: &Namespace, peer: Url) -> Result<CSourceRegistration<Input>> {
    let entities = [
        EntityInfo::new(namespace.entity_type("Vehicle")?).with_id_pattern(namespace.id_pattern("Vehicle")?),
        EntityInfo::new(namespace.entity_type("Parking")?).with_id_pattern(namespace.id_pattern("Parking")?),
    ];
    register(broker_e, peer, namespace.registration_id(1)?, entities).await
}

/// Registers `endpoint` on `broker_e` for the Parkings of the test alone, as registration 2.
///
/// # Errors
///
/// Fails when the registration cannot be built or the broker refuses it.
pub async fn register_parking_source(broker_e: &BrokerClient, namespace: &Namespace, endpoint: Url) -> Result<CSourceRegistration<Input>> {
    let entities = [EntityInfo::new(namespace.entity_type("Parking")?).with_id_pattern(namespace.id_pattern("Parking")?)];
    register(broker_e, endpoint, namespace.registration_id(2)?, entities).await
}

/// Builds the registration of `entities` offering the temporal read operations, registers it and returns it.
async fn register<const N: usize>(broker_e: &BrokerClient, endpoint: Url, id: RegistrationId, entities: [EntityInfo; N]) -> Result<CSourceRegistration<Input>> {
    let registration = CSourceRegistration::builder(endpoint)
        .id(id)
        .information([RegistrationInfo::builder().entities(entities).build()?])
        .operations(OPERATIONS)
        .build()?;
    broker_e.registrations().register(&registration).send().await?;
    Ok(registration)
}

/// Runs the between 12:00-13:00 query on `broker_e` for the Vehicles of the test under `splitEntities`, filtered by
/// `parkedIn{free}>0` with the Linked Entity joined inline one level deep, and requires it to answer 200 with the one
/// Vehicle `selected`.
///
/// # Errors
///
/// Fails when the query cannot be built or sent.
pub async fn expect_hop_selects(broker_e: &BrokerClient, namespace: &Namespace, selected: &EntityId) -> Result<()> {
    let temporal_q = temporal_query::between("2020-08-01T12:00:00Z".parse()?, "2020-08-01T13:00:00Z".parse()?).build()?;
    let selector = SelectorTypeBuilder::from(&namespace.entity_type("Vehicle")?).build()?;
    let linked_q = q::attr("parkedIn").linked("free").gt(0).build()?;
    let query = QueryTemporalEvolution::builder()
        .temporal_q(&temporal_q)
        .entity_type(&selector)
        .q(&linked_q)
        .split_entities(EntitySplitting::Split)
        .format(Lossless::Normalized)
        .build();
    // The temporal query writes no `join`, so the parameters follow the query the operation writes.
    let request = with_join(broker_e.with_accept(Accept::new(NgsiLdMediaType::Json)).unchecked(&query)?, Join::Inline, 1)?;
    let answer = broker_e.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    let response = answer?;
    expect_that!(response.body(), json_count("$", 1));
    expect_that!(response.body(), json_equals("$[0].id", selected.to_string()));
    Ok(())
}
