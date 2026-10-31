//! Annex C.7: an Entity is rendered with the terms of the `@context` it is retrieved under, whatever `@context` it
//! was written under, and the answer `@context` is that `@context` followed by the core one (clauses 5.5.7 and 6.3.6).

use crate::{
    printed_document::{expect_context_only, expect_with_any_context},
    served_context::{ServedContexts, core_context, linked, retrieved_ld},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
};
use googletest::prelude::*;
use ngsi_ld::model::{DateTime, Entity, EntityId, Property};
use serde_json::json;
use url::Url;

/// Serves the parking `@context` the Entity is written under and the abbreviated one naming the same IRIs
/// differently, creates Downtown1 under the first, and answers its id and the abbreviated `@context`'s URL.
async fn parking_written(served: &ServedContexts, namespace: &Namespace, front: &BrokerClient) -> Result<(EntityId, Url)> {
    let parking = namespace.suffixed("OffStreetParking");
    let iri = format!("http://example.org/parking/{parking}");
    let written = served
        .serve(
            "parking-c7.jsonld",
            &json!({"@context": {
                parking.as_str(): iri,
                "availableSpotNumber": "http://example.org/parking/availableSpotNumber",
                "totalSpotNumber": "http://example.org/parking/totalSpotNumber",
                "name": "http://example.org/parking/name",
            }}),
        )
        .await?;
    let abbreviated = served
        .serve(
            "parking-abbreviated.jsonld",
            &json!({"@context": {
                namespace.suffixed("OffP"): iri,
                "ava": "http://example.org/parking/availableSpotNumber",
                "total": "http://example.org/parking/totalSpotNumber",
            }}),
        )
        .await?;
    let entity = Entity::builder()
        .id(namespace.urn("OffStreetParking", "Downtown1")?)
        .entity_type(namespace.entity_type("OffStreetParking")?)
        .property("name", "Downtown One")
        .property(
            "availableSpotNumber",
            Property::builder(121).observed_at("2017-07-29T12:05:02Z".parse::<DateTime>()?),
        )
        .property("totalSpotNumber", 200)
        .build()?;
    linked(front, &written)?.entities().create(&entity).send().await?;
    Ok((entity.id, abbreviated))
}

/// Pins: The `OffStreetParking` written under one vocabulary and retrieved under another naming the same IRIs
/// differently is rendered with the retrieval @context terms, name falling back to its Fully Qualified Name
/// Clause: C.7, 5.5.7, 5.7.1
/// Expected: name is the Fully Qualified Name, the abbreviated @context defining no name term (clause 5.5.7)
#[gtest]
#[tokio::test]
async fn c_7_0001_retrieval_renders_terms_of_the_retrieval_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let front = broker::front()?;
    let (id, abbreviated) = parking_written(&served, &namespace, &front).await?;
    let body = retrieved_ld(&linked(&front, &abbreviated)?, &id).await?;
    expect_with_any_context(
        body,
        &json!({
            "id": id.to_string(),
            "type": namespace.suffixed("OffP"),
            "http://example.org/parking/name": {"type": "Property", "value": "Downtown One"},
            "ava": {"type": "Property", "value": 121, "observedAt": "2017-07-29T12:05:02Z"},
            "total": {"type": "Property", "value": 200},
        }),
    );
    served.stop();
    Ok(())
}

/// Pins: Retrieved under the abbreviated @context, the answer @context holds that @context and then the Core
/// @context, which the abbreviated one does not import
/// Clause: C.7, 5.2.3, 6.3.6, 5.7.1
#[gtest]
#[tokio::test]
async fn c_7_0002_the_response_context_is_the_header_context_then_the_core() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let front = broker::front()?;
    let (id, abbreviated) = parking_written(&served, &namespace, &front).await?;
    let body = retrieved_ld(&linked(&front, &abbreviated)?, &id).await?;
    expect_that!(body.get("@context"), some(eq(&json!([abbreviated.as_str(), core_context()?.as_str()]))));
    served.stop();
    Ok(())
}

/// Pins: Retrieved with no @context, the `OffStreetParking` user terms are rendered as Fully Qualified Names while
/// the core terms stay short, and the answer @context is the Core @context alone
/// Clause: C.7, 4.4, 5.5.7, 6.3.6, 5.7.1
#[gtest]
#[tokio::test]
async fn c_7_0003_no_context_renders_user_names_fully_qualified() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let front = broker::front()?;
    let (id, _) = parking_written(&served, &namespace, &front).await?;
    let body = retrieved_ld(&front, &id).await?;
    expect_context_only(&body, core_context()?.as_str());
    expect_with_any_context(
        body,
        &json!({
            "id": id.to_string(),
            "type": format!("http://example.org/parking/{}", namespace.suffixed("OffStreetParking")),
            "http://example.org/parking/name": {"type": "Property", "value": "Downtown One"},
            "http://example.org/parking/availableSpotNumber": {"type": "Property", "value": 121, "observedAt": "2017-07-29T12:05:02Z"},
            "http://example.org/parking/totalSpotNumber": {"type": "Property", "value": 200},
        }),
    );
    served.stop();
    Ok(())
}
