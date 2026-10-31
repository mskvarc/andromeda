//! The Vehicles and Buses the entity and temporal tests set up before they run and delete after.

use crate::{
    entity_operations::create_entity,
    temporal_operations::{create_temporal_entity, create_temporal_evolution, delete_temporal_entities},
    vehicle_entity_payloads::{vehicle_brand_parked_speed_gps, vehicle_brand_parked_speed_sources, vehicle_parked_speed_sources},
    vehicle_evolution_payloads::{
        VEHICLE_SPEED_FUEL_TERMS,
        bus_speed_fuel_level,
        vehicle_fuel_level,
        vehicle_parking_name_fuel,
        vehicle_speed_fuel_level,
        vehicle_speed_fuel_level_on,
        vehicle_speed_fuel_level_with_dataset,
        vehicle_twenty_instances,
    },
};
use andromeda_test_suite::{broker::BrokerClient, namespace::Namespace, problem::answered, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, operation::CreateOrUpdateTemporalEvolution},
    model::{Lossless, RawJson},
};

/// The setup of the temporal tests that load the Vehicle of [`vehicle_speed_fuel_level`]: registers the terms
/// `Vehicle`, `speed` and `fuelLevel`, creates that Vehicle in their context, and answers the vocabulary.
pub(crate) async fn create_vehicle_speed_fuel_level(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    create_temporal_evolution(broker, namespace, &VEHICLE_SPEED_FUEL_TERMS, &vehicle_speed_fuel_level(namespace)?).await
}

/// The setup of the temporal tests that load the Vehicle of [`vehicle_speed_fuel_level_with_dataset`]: registers the
/// terms `Vehicle`, `speed` and `fuelLevel`, creates that Vehicle in their context, and answers the vocabulary.
pub(crate) async fn create_vehicle_speed_fuel_level_with_dataset(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    create_temporal_evolution(broker, namespace, &VEHICLE_SPEED_FUEL_TERMS, &vehicle_speed_fuel_level_with_dataset(namespace)?).await
}

/// The setup of the replace tests: registers `terms` (the test's context, at least the terms of
/// `VEHICLE_PARKED_SPEED_SOURCE_TERMS`), and creates the Vehicle of [`vehicle_parked_speed_sources`] in their context
/// by a create that requires 201, answering the vocabulary.
pub(crate) async fn create_vehicle_parked_speed_sources(broker: &BrokerClient, namespace: &Namespace, terms: &[&str]) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, terms).await?;
    create_entity(broker, &vocabulary, &vehicle_parked_speed_sources(namespace)?).await?;
    Ok(vocabulary)
}

/// The setup of the delete tests that load the Vehicle of [`vehicle_brand_parked_speed_gps`]: registers `terms` (the
/// test's context, at least the terms of `VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS`), creates that Vehicle in their
/// context by a create that requires 201, and answers the vocabulary.
pub(crate) async fn create_vehicle_brand_parked_speed_gps(broker: &BrokerClient, namespace: &Namespace, terms: &[&str]) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, terms).await?;
    create_entity(broker, &vocabulary, &vehicle_brand_parked_speed_gps(namespace)?).await?;
    Ok(vocabulary)
}

/// The setup of the append and partial-update tests that load the Vehicle of [`vehicle_brand_parked_speed_sources`]:
/// registers `terms`, creates that Vehicle in their context by a create that requires 201, and answers the vocabulary.
pub(crate) async fn create_vehicle_brand_parked_speed_sources(broker: &BrokerClient, namespace: &Namespace, terms: &[&str]) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, terms).await?;
    create_entity(broker, &vocabulary, &vehicle_brand_parked_speed_sources(namespace)?).await?;
    Ok(vocabulary)
}

/// The setup of the scope tests: registers the terms `Vehicle` and `fuelLevel`, and creates the Vehicle of
/// [`vehicle_fuel_level`] in their context with `scope` as the list of the two scope names. The typed
/// `scope` member writes a Property that the broker keeps only beside an `observedAt`, so the body is the rendered
/// evolution with that list written in its place. Requires 201.
pub(crate) async fn create_vehicle_scoped_fuel(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &["Vehicle", "fuelLevel"]).await?;
    let evolution = vehicle_fuel_level(namespace)?;
    let scope: RawJson = r#"["/Madrid/Gardens/ParqueNorte","/CompanyA/OrganizationB/UnitC"]"#.parse()?;
    let document = UncheckedDocument::try_from(evolution.render(Lossless::Normalized))?.set_member("scope", &scope);
    let client = broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let operation = CreateOrUpdateTemporalEvolution::builder().entity(&evolution).build();
    let request = client.unchecked(&operation)?.document(&document);
    let answer = client.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::CREATED))?;
    Ok(vocabulary)
}

/// The setup of the temporal tests that load the Vehicle of [`vehicle_parking_name_fuel`]: registers the terms
/// `Vehicle`, `isParkedIn`, `name` and `fuelLevel`, creates that Vehicle in their context, and answers the vocabulary.
pub(crate) async fn create_vehicle_parking_name_fuel(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    create_temporal_evolution(
        broker,
        namespace,
        &["Vehicle", "isParkedIn", "name", "fuelLevel"],
        &vehicle_parking_name_fuel(namespace)?,
    )
    .await
}

/// The setup of the tests that load Vehicles `<suffix>-A` observed on 2020-08-01 and `<suffix>-B` observed on
/// 2020-09-01: registers the terms `Vehicle`, `speed` and `fuelLevel`, creates both in their context, and answers the
/// vocabulary.
pub(crate) async fn create_vehicle_pair(broker: &BrokerClient, namespace: &Namespace, suffix: &str) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &VEHICLE_SPEED_FUEL_TERMS).await?;
    create_vehicle_entities(broker, namespace, &vocabulary, suffix).await?;
    Ok(vocabulary)
}

/// The setup of the tests that load the Vehicle pair of [`create_vehicle_pair`] and Bus `<bus_suffix>` observed on
/// 2020-08-01: registers the terms `Vehicle`, `speed`, `fuelLevel` and `Bus`, creates the three in their context, and
/// answers the vocabulary.
pub(crate) async fn create_vehicle_pair_and_bus(broker: &BrokerClient, namespace: &Namespace, suffix: &str, bus_suffix: &str) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &["Vehicle", "speed", "fuelLevel", "Bus"]).await?;
    create_vehicle_pair_and_bus_in(broker, &vocabulary, namespace, suffix, bus_suffix).await?;
    Ok(vocabulary)
}

/// The entities of [`create_vehicle_pair_and_bus`], created in the context of `vocabulary` for a case whose vocabulary
/// names further terms.
pub(crate) async fn create_vehicle_pair_and_bus_in(
    broker: &BrokerClient,
    vocabulary: &Vocabulary,
    namespace: &Namespace,
    suffix: &str,
    bus_suffix: &str,
) -> Result<()> {
    create_vehicle_entities(broker, namespace, vocabulary, suffix).await?;
    create_temporal_entity(broker, vocabulary, &bus_speed_fuel_level(namespace, bus_suffix)?).await
}

/// Creates Vehicle `<suffix>-A` observed on 2020-08-01 and Vehicle `<suffix>-B` observed on 2020-09-01 in the context
/// of `vocabulary`.
async fn create_vehicle_entities(broker: &BrokerClient, namespace: &Namespace, vocabulary: &Vocabulary, suffix: &str) -> Result<()> {
    create_temporal_entity(
        broker,
        vocabulary,
        &vehicle_speed_fuel_level_on(namespace, format!("{suffix}-A"), "2020-08-01")?,
    )
    .await?;
    create_temporal_entity(
        broker,
        vocabulary,
        &vehicle_speed_fuel_level_on(namespace, format!("{suffix}-B"), "2020-09-01")?,
    )
    .await
}

/// The teardown of the cases of [`create_vehicle_pair`]: deletes the temporal evolutions of Vehicles `<suffix>-A` and
/// `<suffix>-B`.
pub(crate) async fn delete_vehicle_pair(broker: &BrokerClient, namespace: &Namespace, suffix: &str) -> Result<()> {
    delete_temporal_entities(
        broker,
        &[
            &namespace.urn("Vehicle", format!("{suffix}-A"))?,
            &namespace.urn("Vehicle", format!("{suffix}-B"))?,
        ],
    )
    .await;
    Ok(())
}

/// The teardown of the cases of [`create_vehicle_pair_and_bus`]: deletes the temporal evolutions of Vehicles
/// `<suffix>-A` and `<suffix>-B` and of Bus `<bus_suffix>`.
pub(crate) async fn delete_vehicle_pair_and_bus(broker: &BrokerClient, namespace: &Namespace, suffix: &str, bus_suffix: &str) -> Result<()> {
    delete_vehicle_pair(broker, namespace, suffix).await?;
    delete_temporal_entities(broker, &[&namespace.urn("Bus", bus_suffix)?]).await;
    Ok(())
}

/// The setup of the temporal query tests that page through the Vehicle `<ns>-1` of [`vehicle_twenty_instances`] and the
/// Vehicle `<ns>-2` observed on 2020-09-01: registers the terms `Vehicle`, `speed` and `fuelLevel`, creates both
/// Vehicles in their context, and answers the vocabulary.
pub(crate) async fn create_vehicle_twenty_pair(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &VEHICLE_SPEED_FUEL_TERMS).await?;
    create_temporal_entity(broker, &vocabulary, &vehicle_twenty_instances(namespace)?).await?;
    create_temporal_entity(broker, &vocabulary, &vehicle_speed_fuel_level_on(namespace, 2, "2020-09-01")?).await?;
    Ok(vocabulary)
}

/// The teardown of the cases of [`create_vehicle_twenty_pair`]: deletes the temporal evolutions of Vehicles `<ns>-1`
/// and `<ns>-2`.
pub(crate) async fn delete_vehicle_twenty_pair(broker: &BrokerClient, namespace: &Namespace) -> Result<()> {
    delete_temporal_entities(broker, &[&namespace.urn("Vehicle", 1)?, &namespace.urn("Vehicle", 2)?]).await;
    Ok(())
}
