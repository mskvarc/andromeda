//! The chains of Buildings, Cities, Countries and Vehicles the linked-entity tests set up before they run and delete
//! after.

use crate::{
    building_payloads::{building_simple_attributes, building_street_located, pisa_tower_sightseeing},
    entity_operations::{create_entity, delete_entities},
    linked_place_payloads::{
        building_located_at_city,
        building_located_in_city,
        building_located_in_paris,
        city,
        city_in_france_described,
        city_paris_in_country,
        city_paris_in_france,
        country_described,
        country_france_described,
    },
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker::BrokerClient, namespace::Namespace, vocabulary::Vocabulary};
use googletest::Result;
use ngsi_ld::model::{Entity, EntityId, Input};

/// The setup of the linked-entity tests: creates `urn:ngsi-ld:Building:<ns>-<suffix>` located at the City Paris, the
/// City Paris located in France and the Country France, in `vocabulary`'s context, and answers their identifiers in
/// that order.
pub(crate) async fn create_paris_in_france(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace, suffix: &str) -> Result<[EntityId; 3]> {
    create_entity(broker, vocabulary, &building_located_in_paris(namespace, suffix)?).await?;
    create_entity(broker, vocabulary, &city_paris_in_france(namespace)?).await?;
    create_entity(broker, vocabulary, &country_france_described(namespace)?).await?;
    Ok([
        namespace.urn("Building", suffix)?,
        namespace.urn("City", "Paris")?,
        namespace.urn("Country", "France")?,
    ])
}

/// The setup of the linked-entity tests: creates Building `urn:ngsi-ld:Building:<ns>-<pair>-1` located at City
/// `<pair>-1`, that City, then Building `<pair>-2` located at City `<pair>-2`, and that City, in `vocabulary`'s
/// context.
pub(crate) async fn create_linked_pairs(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace, pair: &str) -> Result<()> {
    create_pairs(broker, vocabulary, namespace, pair, building_located_at_city, city).await
}

/// The setup of the linked tests with a street and a location: creates the Buildings and Cities of
/// [`create_linked_pairs`] with the payloads of `building_street_located` and `city_in_france_described`.
pub(crate) async fn create_street_linked_pairs(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace, pair: &str) -> Result<()> {
    create_pairs(broker, vocabulary, namespace, pair, building_street_located, city_in_france_described).await
}

/// Creates Building `<pair>-1` and City `<pair>-1`, then Building `<pair>-2` and City `<pair>-2`, with the payloads
/// `building_payload` and `city_payload` build for the suffix.
async fn create_pairs(
    broker: &BrokerClient,
    vocabulary: &Vocabulary,
    namespace: &Namespace,
    pair: &str,
    building_payload: fn(&Namespace, &str) -> Result<Entity<Input>>,
    city_payload: fn(&Namespace, &str) -> Result<Entity<Input>>,
) -> Result<()> {
    let first = format!("{pair}-1");
    let second = format!("{pair}-2");
    create_entity(broker, vocabulary, &building_payload(namespace, &first)?).await?;
    create_entity(broker, vocabulary, &city_payload(namespace, &first)?).await?;
    create_entity(broker, vocabulary, &building_payload(namespace, &second)?).await?;
    create_entity(broker, vocabulary, &city_payload(namespace, &second)?).await
}

/// The teardown of the linked-entity cases: deletes the Buildings and Cities that [`create_linked_pairs`] creates for
/// `pair`.
pub(crate) async fn delete_linked_pairs(broker: &BrokerClient, namespace: &Namespace, pair: &str) -> Result<()> {
    let first = format!("{pair}-1");
    let second = format!("{pair}-2");
    delete_entities(
        broker,
        &[
            &namespace.urn("Building", &first)?,
            &namespace.urn("City", &first)?,
            &namespace.urn("Building", &second)?,
            &namespace.urn("City", &second)?,
        ],
    )
    .await;
    Ok(())
}

/// The setup of the Building and Vehicle tests: creates Building `urn:ngsi-ld:Building:<ns>-1` named Eiffel Tower,
/// Vehicle `urn:ngsi-ld:Vehicle:<ns>-2` parked downtown and Building `urn:ngsi-ld:Building:<ns>-3` named Pisa Tower, in
/// `vocabulary`'s context.
pub(crate) async fn create_eiffel_vehicle_pisa(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace) -> Result<()> {
    create_entity(broker, vocabulary, &building_simple_attributes(namespace, 1)?).await?;
    create_entity(broker, vocabulary, &vehicle_parked(namespace, 2)?).await?;
    create_entity(broker, vocabulary, &pisa_tower_sightseeing(namespace)?).await
}

/// The teardown of the Building and Vehicle cases: deletes the entities [`create_eiffel_vehicle_pisa`] creates.
pub(crate) async fn delete_eiffel_vehicle_pisa(broker: &BrokerClient, namespace: &Namespace) -> Result<()> {
    delete_entities(
        broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Vehicle", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}

/// The setup of the Building, City and Country tests: creates two chains, `<pair>-1` and `<pair>-2`, each a Building
/// `urn:ngsi-ld:Building:<ns>-<suffix>` located at City `<suffix>`, that City named Paris and located in Country
/// `<suffix>` named France, in `vocabulary`'s context.
pub(crate) async fn create_building_city_country_pairs(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace, pair: &str) -> Result<()> {
    for suffix in [format!("{pair}-1"), format!("{pair}-2")] {
        create_entity(broker, vocabulary, &building_located_in_city(namespace, &suffix, &suffix)?).await?;
        create_entity(broker, vocabulary, &city_paris_in_country(namespace, &suffix, &suffix)?).await?;
        create_entity(broker, vocabulary, &country_described(namespace, &suffix)?).await?;
    }
    Ok(())
}

/// The teardown of the cases of [`create_building_city_country_pairs`]: deletes the Buildings, Cities and Countries
/// that it creates for `pair`.
pub(crate) async fn delete_building_city_country_pairs(broker: &BrokerClient, namespace: &Namespace, pair: &str) -> Result<()> {
    for suffix in [format!("{pair}-1"), format!("{pair}-2")] {
        delete_entities(
            broker,
            &[
                &namespace.urn("Building", &suffix)?,
                &namespace.urn("City", &suffix)?,
                &namespace.urn("Country", &suffix)?,
            ],
        )
        .await;
    }
    Ok(())
}
