//! Annex C.9: a term both a user vocabulary and the core `@context` define expands through the core `@context`,
//! whatever its position, a prefixed term reaches the user definition, and the answer `@context` ends with the core
//! one (clauses 4.4 and 5.5.7).

use crate::{
    printed_document::expect_context_only,
    served_context::{ServedContexts, core_context, linked, retrieved_document, retrieved_ld},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateEntity},
    model::{Entity, EntityId, Input, Lossless, RawJson},
};
use serde_json::{Value, json};
use url::Url;

/// The street address of the Empire State building.
const ADDRESS: &str = "20 West 34th Street, New York City, NY 10001";

/// Serves the schema stand-in and answers its URL.
async fn schema(served: &ServedContexts) -> Result<Url> {
    served
        .serve(
            "schema-stand-in.jsonld",
            &json!({"@context": {
                "schema": "https://schema.org/",
                "location": "https://schema.org/location",
                "name": "https://schema.org/name",
            }}),
        )
        .await
}

/// The term object defining the Building type.
fn building_term(namespace: &Namespace) -> Value {
    let building = namespace.suffixed("Building");
    json!({building.as_str(): format!("http://example.org/building/{building}")})
}

/// Serves the colliding `@context`: the schema stand-in, the parking vocabulary, the Building term, then the core
/// `@context`; answers its URL.
async fn colliding_context(served: &ServedContexts, namespace: &Namespace) -> Result<Url> {
    let schema = schema(served).await?;
    let parking = namespace.suffixed("OffStreetParking");
    let parking = served
        .serve(
            "parking.jsonld",
            &json!({"@context": {
                parking.as_str(): format!("http://example.org/parking/{parking}"),
                "availableSpotNumber": "http://example.org/parking/availableSpotNumber",
                "totalSpotNumber": "http://example.org/parking/totalSpotNumber",
            }}),
        )
        .await?;
    served
        .serve(
            "colliding-context.jsonld",
            &json!({"@context": [schema.as_str(), parking.as_str(), building_term(namespace), core_context()?.as_str()]}),
        )
        .await
}

/// The Building B1, the Empire State, its address in `address_term`, unless no term is given.
fn building(namespace: &Namespace, address_term: Option<&str>) -> Result<Entity<Input>> {
    let builder = Entity::builder()
        .id(namespace.urn("Building", "B1")?)
        .entity_type(namespace.entity_type("Building")?)
        .property("name", "Empire State");
    Ok(match address_term {
        Some(term) => builder.property(term, ADDRESS).build()?,
        None => builder.build()?,
    })
}

/// Creates B1 with its address in the core term `location`, through the client sending `context`, requiring 201,
/// and answers its id.
async fn created_with_location(namespace: &Namespace, context: &Url) -> Result<EntityId> {
    let client = linked(&broker::front()?, context)?;
    let building = building(namespace, None)?;
    // The builder admits only a GeoProperty as `location`; the example writes a Property there.
    let location: RawJson = json!({"type": "Property", "value": ADDRESS}).to_string().parse()?;
    let document = UncheckedDocument::from_entity(&building, Lossless::Normalized).set_member("location", &location);
    let request = client.unchecked(&CreateEntity::builder().entity(&building).build())?.document(&document);
    let created = client.send_unchecked(request).await;
    verify_that!(&created, answered(StatusCode::CREATED))?;
    Ok(building.id)
}

/// Pins: A Building written under an @context array whose schema.org vocabulary also defines location has location
/// expanded through the Core @context, while name takes the schema.org IRI
/// Clause: C.9, 4.4, 5.5.7, 5.7.1
#[gtest]
#[tokio::test]
async fn c_9_0001_a_colliding_term_expands_through_the_core_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let id = created_with_location(&namespace, &colliding_context(&served, &namespace).await?).await?;
    let body = retrieved_document(&broker::front()?, &id).await?;
    expect_that!(body.pointer("/location/value"), some(eq(&json!(ADDRESS))));
    expect_that!(body.get("https://schema.org/location"), none());
    expect_that!(body.pointer("/https:~1~1schema.org~1name/value"), some(eq(&json!("Empire State"))));
    served.stop();
    Ok(())
}

/// Pins: With the Core @context first in the array, location still expands through it and not through the
/// schema.org vocabulary after it
/// Clause: C.9, 4.4, 5.5.7, 5.7.1
#[gtest]
#[tokio::test]
async fn c_9_0002_the_core_context_wins_from_the_first_position() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let schema = schema(&served).await?;
    let core_first = served
        .serve(
            "colliding-context-core-first.jsonld",
            &json!({"@context": [core_context()?.as_str(), schema.as_str(), building_term(&namespace)]}),
        )
        .await?;
    let id = created_with_location(&namespace, &core_first).await?;
    let body = retrieved_document(&broker::front()?, &id).await?;
    expect_that!(body.pointer("/location/value"), some(eq(&json!(ADDRESS))));
    expect_that!(body.get("https://schema.org/location"), none());
    served.stop();
    Ok(())
}

/// Pins: Under the same colliding @context, schema:location expands to the schema.org location IRI the collision
/// hides
/// Clause: C.9, 5.5.7, 5.7.1
#[gtest]
#[tokio::test]
async fn c_9_0003_a_prefixed_term_reaches_the_user_definition() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let building = building(&namespace, Some("schema:location"))?;
    linked(&broker::front()?, &colliding_context(&served, &namespace).await?)?
        .entities()
        .create(&building)
        .send()
        .await?;
    let id = building.id;
    let body = retrieved_ld(&broker::front()?, &id).await?;
    expect_that!(
        body.get("https://schema.org/location"),
        some(eq(&json!({"type": "Property", "value": ADDRESS})))
    );
    served.stop();
    Ok(())
}

/// Pins: The answer @context of a retrieval ends with the Core @context, the position C.9 requires implementations
/// to render it in
/// Clause: C.9, 4.4, 6.3.6, 5.7.1
#[gtest]
#[tokio::test]
async fn c_9_0004_the_rendered_context_ends_with_the_core_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let id = created_with_location(&namespace, &colliding_context(&served, &namespace).await?).await?;
    let body = retrieved_document(&broker::front()?, &id).await?;
    expect_context_only(&body, core_context()?.as_str());
    served.stop();
    Ok(())
}
