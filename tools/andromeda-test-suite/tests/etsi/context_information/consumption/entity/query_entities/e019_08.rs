//! Entities are selected by an entity type selection expression: one type, the AND of two types, the OR of types, and
//! expressions with two operators (clauses 4.17 and 5.7.2).

use crate::entity_operations::{create_entity, delete_entities};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::json_body,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Input, Property},
    query::SelectorType,
};
use serde_json::Value as Json;

/// The terms of the `@context` of every case.
const TERMS: [&str; 7] = [
    "Building",
    "name",
    "subCategory",
    "airQualityLevel",
    "almostFull",
    "Parking",
    "TouristDestination",
];

/// `urn:ngsi-ld:MultiTypes:<ns>-1`: a Building named Eiffel Tower, sub-categorised as tourism, with an `airQualityLevel`
/// of 4 in the unit C62 observed at 2020-09-09T16:40:00.000Z and an `almostFull` of false.
fn building(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("MultiTypes", 1)?)
        .entity_type("Building")
        .property("name", "Eiffel Tower")
        .property("subCategory", "tourism")
        .property(
            "airQualityLevel",
            Property::builder(4).unit_code(UnitCode::C62).observed_at("2020-09-09T16:40:00.000Z".parse()?),
        )
        .property("almostFull", false)
        .build()?)
}

/// `urn:ngsi-ld:MultiTypes:<ns>-2`: a Parking, with no attribute.
fn parking(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder().id(namespace.urn("MultiTypes", 2)?).entity_type("Parking").build()?)
}

/// `urn:ngsi-ld:MultiTypes:<ns>-3`: a Building and a `TouristDestination`, with no attribute.
fn building_and_tourist_destination(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("MultiTypes", 3)?)
        .entity_type(["Building", "TouristDestination"])
        .build()?)
}

/// Creates the three entities every case queries, in `vocabulary`'s context.
async fn create_all(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace) -> Result<()> {
    create_entity(broker, vocabulary, &building(namespace)?).await?;
    create_entity(broker, vocabulary, &parking(namespace)?).await?;
    create_entity(broker, vocabulary, &building_and_tourist_destination(namespace)?).await
}

/// Queries the entities whose type matches `selection` under the case's `Link` context, without an `Accept` header, and
/// answers the body.
async fn selected(broker: &BrokerClient, vocabulary: &Vocabulary, selection: &str) -> Result<Json> {
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type(selection.parse::<SelectorType>()?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    Ok(json_body(&received?)?)
}

/// The ids of the entities an answer lists, in the order it lists them.
fn listed_ids(body: &Json) -> Vec<&str> {
    let mut ids: Vec<&str> = body.as_array().into_iter().flatten().filter_map(|entity| entity.get("id")?.as_str()).collect();
    ids.sort_unstable();
    ids
}

/// Pins: Query entities with Entity Type Selection Language.
/// Clause: 4.17, 5.7.2
#[gtest]
#[tokio::test]
async fn e019_08_01_query_with_one_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_all(&broker, &vocabulary, &namespace).await?;
    let body = selected(&broker, &vocabulary, "Building").await?;
    let first = namespace.urn_text("MultiTypes", 1);
    let third = namespace.urn_text("MultiTypes", 3);
    expect_that!(listed_ids(&body), eq(&vec![first.as_str(), third.as_str()]));
    delete_entities(
        &broker,
        &[
            &namespace.urn("MultiTypes", 1)?,
            &namespace.urn("MultiTypes", 2)?,
            &namespace.urn("MultiTypes", 3)?,
        ],
    )
    .await;
    Ok(())
}

/// Pins: Query entities with Entity Type Selection Language.
/// Clause: 4.17, 5.7.2
#[gtest]
#[tokio::test]
async fn e019_08_02_query_with_the_and_operator() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_all(&broker, &vocabulary, &namespace).await?;
    let body = selected(&broker, &vocabulary, "(Building;TouristDestination)").await?;
    let third = namespace.urn_text("MultiTypes", 3);
    expect_that!(listed_ids(&body), eq(&vec![third.as_str()]));
    delete_entities(
        &broker,
        &[
            &namespace.urn("MultiTypes", 1)?,
            &namespace.urn("MultiTypes", 2)?,
            &namespace.urn("MultiTypes", 3)?,
        ],
    )
    .await;
    Ok(())
}

/// Pins: Query entities with Entity Type Selection Language.
/// Clause: 4.17, 5.7.2
#[gtest]
#[tokio::test]
async fn e019_08_03_query_with_the_or_operator() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_all(&broker, &vocabulary, &namespace).await?;
    let body = selected(&broker, &vocabulary, "Building,Parking").await?;
    let first = namespace.urn_text("MultiTypes", 1);
    let second = namespace.urn_text("MultiTypes", 2);
    let third = namespace.urn_text("MultiTypes", 3);
    expect_that!(listed_ids(&body), eq(&vec![first.as_str(), second.as_str(), third.as_str()]));
    delete_entities(
        &broker,
        &[
            &namespace.urn("MultiTypes", 1)?,
            &namespace.urn("MultiTypes", 2)?,
            &namespace.urn("MultiTypes", 3)?,
        ],
    )
    .await;
    Ok(())
}

/// Pins: Query entities with Entity Type Selection Language.
/// Clause: 4.17, 5.7.2
#[gtest]
#[tokio::test]
async fn e019_08_04_different_query_with_the_or_operator() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_all(&broker, &vocabulary, &namespace).await?;
    let body = selected(&broker, &vocabulary, "Parking,TouristDestination").await?;
    let second = namespace.urn_text("MultiTypes", 2);
    let third = namespace.urn_text("MultiTypes", 3);
    expect_that!(listed_ids(&body), eq(&vec![second.as_str(), third.as_str()]));
    delete_entities(
        &broker,
        &[
            &namespace.urn("MultiTypes", 1)?,
            &namespace.urn("MultiTypes", 2)?,
            &namespace.urn("MultiTypes", 3)?,
        ],
    )
    .await;
    Ok(())
}

/// Pins: Query entities with Entity Type Selection Language.
/// Clause: 4.17, 5.7.2
#[gtest]
#[tokio::test]
async fn e019_08_05_query_with_two_operators() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_all(&broker, &vocabulary, &namespace).await?;
    let body = selected(&broker, &vocabulary, "(Building;Parking),TouristDestination").await?;
    let third = namespace.urn_text("MultiTypes", 3);
    expect_that!(listed_ids(&body), eq(&vec![third.as_str()]));
    delete_entities(
        &broker,
        &[
            &namespace.urn("MultiTypes", 1)?,
            &namespace.urn("MultiTypes", 2)?,
            &namespace.urn("MultiTypes", 3)?,
        ],
    )
    .await;
    Ok(())
}
