//! A query whose ids, id pattern, attribute names, geometry property or minimal parameters are incorrect is refused
//! with 400 `BadRequestData` (clause 5.7.2).

use crate::{
    building_payloads::{bare_building, eiffel_tower_located},
    entity_operations::{context_link, create_entity, delete_entities},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, vocabulary::Vocabulary, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode, header::LINK};
use ngsi_ld::model::ErrorType;

/// The `type` of the cases' Buildings, as the query writes it: the expanded IRI of `Building` in `namespace`.
fn building_type_query(namespace: &Namespace) -> String {
    format!("type=https%3A%2F%2Ftest.andromeda%2F{}%2FBuilding", namespace.token())
}

/// Pins: Check that one cannot query entities if the requested ids are incorrect
/// Clause: 5.7.2
#[gtest]
#[tokio::test]
async fn e019_03_01_query_entities_based_on_incorrect_ids() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    create_entity(&broker, &vocabulary, &bare_building(&namespace, 1)?).await?;
    create_entity(&broker, &vocabulary, &bare_building(&namespace, 2)?).await?;
    let target = format!("entities?id=thisisaninvaliduri1%2Cthisisaninvaliduri2&{}", building_type_query(&namespace));
    let answer = broker.send_unchecked(request_to(&broker, Method::GET, &target)?).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one cannot query entities if the requested id pattern is incorrect
/// Clause: 5.7.2
#[gtest]
#[tokio::test]
async fn e019_03_02_query_several_entities_based_on_incorrect_id_pattern() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    create_entity(&broker, &vocabulary, &bare_building(&namespace, 1)?).await?;
    create_entity(&broker, &vocabulary, &bare_building(&namespace, 2)?).await?;
    let target = format!("entities?{}&idPattern=invalid_entity_id_pattern%2A%2A", building_type_query(&namespace));
    let answer = broker.send_unchecked(request_to(&broker, Method::GET, &target)?).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one cannot query entities if the requested attribute names are incorrect
/// Clause: 5.7.2
#[gtest]
#[tokio::test]
async fn e019_03_03_query_several_entities_based_on_incorrect_attribute_names() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    create_entity(&broker, &vocabulary, &bare_building(&namespace, 1)?).await?;
    create_entity(&broker, &vocabulary, &bare_building(&namespace, 2)?).await?;
    let answer = broker.send_unchecked(request_to(&broker, Method::GET, "entities?attrs=id%2Ctype")?).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one cannot query entities if the request has a wrong geometryProperty
/// Clause: 5.7.2
#[gtest]
#[tokio::test]
async fn e019_03_04_query_entities_when_the_request_has_a_wrong_geometry_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name", "subCategory"]).await?;
    create_entity(&broker, &vocabulary, &eiffel_tower_located(&namespace, 1)?).await?;
    create_entity(&broker, &vocabulary, &eiffel_tower_located(&namespace, 2)?).await?;
    let request = request_to(
        &broker,
        Method::GET,
        "entities?geoproperty=location&type=Building&georel=invalid&coordinates=%5B-8.503%2C41.202%5D&geometry=Point",
    )?
    .append_header(LINK, context_link(&vocabulary)?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one cannot query entities if it has none of the minimal parameters
/// Clause: 5.7.2
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e019_03_05_query_entities_without_minimal_parameters() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let target = format!(
        "entities?id=urn%3Angsi-ld%3AEntity%3A{0}-01%2Curn%3Angsi-ld%3AEntity%3A{0}-02",
        namespace.token()
    );
    let answer = broker.send_unchecked(request_to(&broker, Method::GET, &target)?).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    Ok(())
}
