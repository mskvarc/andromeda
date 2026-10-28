//! ETSI `020_08`: the temporal evolution of an entity is refused with 404 `ResourceNotFound` when the attribute
//! asked for is one the vocabulary names but the entity does not hold (clause 5.7.3).

use crate::{suite_keywords::create_temporal_entity, suite_payloads::vehicle_speed_fuel_level};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::Accept, model::ErrorType};

/// Pins: Check that one cannot retrieve the temporal evolution of non-existing entity attributes
/// Clause: 5.7.3
#[gtest]
#[tokio::test]
async fn e020_08_01_retrieve_the_temporal_evolution_of_non_existing_entity_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "speed", "fuelLevel", "unknownAttribute"]).await?;
    create_temporal_entity(&broker, &vocabulary, &vehicle_speed_fuel_level(&namespace)?).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve(&id)
        .attrs(vec!["unknownAttribute".parse()?])
        .send()
        .await;
    verify_that!(&received, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title())?;

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}
