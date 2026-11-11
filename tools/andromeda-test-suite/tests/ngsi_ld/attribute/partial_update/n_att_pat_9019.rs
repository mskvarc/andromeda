//! An ngsildproof on the entity is refused by creation and by a partial update.

use andromeda_test_suite::{broker, entity_matcher::lacks_attribute, namespace::Namespace, outcome::Done, problem::problem, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson, Value},
};

/// The `ngsildproof` both writes carry, sealing the Building it is written onto.
const NGSILDPROOF: &str = r#"{"type": "Property", "entityIdSealed": "urn:ngsi-ld:Building:{namespace}-partial-update", "entityTypeSealed": "Building_{namespace}", "value": {"type": "DataIntegrityProof", "created": "2025-01-27T21:02:24Z", "verificationMethod": "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT", "cryptosuite": "eddsa-rdfc-2022", "proofPurpose": "assertionMethod", "proofValue": "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS"}}"#;

/// Pins: Create Entity whose own attribute is a complete ngsildproof answers 400 `BadRequestData` and creates
/// nothing, and Partial Attribute Update of an attribute named ngsildproof answers 400 `BadRequestData` and leaves the Entity
/// without one: the attribute data types (Table 5.2.5-1 and its siblings) have an `ngsildproof` member and the
/// Entity data type (Table 5.2.4-1) has none
/// Clause: 4.5.2.2, 5.2.4, 5.6.4.4
#[gtest]
#[tokio::test]
async fn n_att_pat_9019_an_ngsildproof_on_the_entity_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_sealed_id = namespace.urn("Building", "partial-update-sealed")?;
    let building_type = namespace.entity_type("Building")?;
    let building_sealed = Entity::builder().id(building_sealed_id.clone()).entity_type(building_type.clone()).build()?;
    let ngsildproof: RawJson = NGSILDPROOF.replace("{namespace}", &namespace.to_string()).parse()?;
    // The model refuses it (an attribute proof), so `ngsildproof` is written as text.
    let document = UncheckedDocument::from_entity(&building_sealed, Lossless::Normalized).set_member("ngsildproof", &ngsildproof);
    let request = broker.unchecked(&CreateEntity::builder().entity(&building_sealed).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_sealed_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let building_id = namespace.urn("Building", "partial-update")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    // The model refuses the seals as members of an attribute, so the patch is sent as written.
    let request_2 = json_request_to(
        &broker,
        Method::PATCH,
        &format!("entities/{building_id}/attrs/ngsildproof"),
        &NGSILDPROOF.replace("{namespace}", &namespace.to_string()),
    )?;
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker.entities().retrieve(&building_id).send().await?;
    expect_that!(retrieved.body(), lacks_attribute("ngsildproof"));
    Ok(())
}
