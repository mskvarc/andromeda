//! An attribute fragment carrying a proof keeps the seals bare.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    bare_seals::bare_seals,
    broker::{self, BrokerClient},
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityId, EntityTypeName, NgsiLdMediaType, Value},
};

/// Reads the entity `id` back and requires its `temperature` to carry the proof sealed to `id` and `entity_type`,
/// the seals bare in the body as received.
async fn reads_back_sealed(broker: &BrokerClient, id: &EntityId, entity_type: &EntityTypeName) -> Result<()> {
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(id)
        .send()
        .await?;
    expect_that!(&received.body, bare_seals());
    expect_that!(
        received.outcome.body(),
        has_attribute(
            "temperature",
            any_attribute()
                .sealed_to(id.clone())
                .sealed_type(entity_type.clone())
                .proof_member(["type"], "DataIntegrityProof")
                .proof_member(["created"], "2025-01-27T21:02:24Z")
                .proof_member(["verificationMethod"], "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT")
                .proof_member(["cryptosuite"], "eddsa-rdfc-2022")
                .proof_member(["proofPurpose"], "assertionMethod")
                .proof_member(["proofValue"], "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS")
        )
    );
    Ok(())
}

/// Pins: Partial Attribute Update and then Replace Attribute of temperature with a fragment carrying an ngsildproof
/// each answer 204, and the proof reads back with bare seals
/// Clause: 4.5.2.2, 5.6.4, 5.6.19
#[gtest]
#[tokio::test]
async fn n_att_pat_9020_an_attribute_fragment_carrying_a_proof_keeps_the_seals_bare() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_carrier_id = namespace.urn("Building", "carrier")?;
    let building_type = namespace.entity_type("Building")?;
    let building_carrier = Entity::builder()
        .id(building_carrier_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_carrier).send().await?.done()?;
    let fragment = format!(
        r#"{{"type": "Property", "value": 22.5, "ngsildproof": {{"type": "Property", "entityIdSealed": "urn:ngsi-ld:Building:{namespace}-carrier", "entityTypeSealed": "Building_{namespace}", "value": {{"type": "DataIntegrityProof", "created": "2025-01-27T21:02:24Z", "verificationMethod": "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT", "cryptosuite": "eddsa-rdfc-2022", "proofPurpose": "assertionMethod", "proofValue": "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS"}}}}}}"#
    );
    // The model refuses an attribute proof, so the fragment is sent as written.
    let request = json_request_to(&broker, Method::PATCH, &format!("entities/{building_carrier_id}/attrs/temperature"), &fragment)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    reads_back_sealed(&broker, &building_carrier_id, &building_type).await?;
    let request_2 = json_request_to(&broker, Method::PUT, &format!("entities/{building_carrier_id}/attrs/temperature"), &fragment)?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::NO_CONTENT));
    reads_back_sealed(&broker, &building_carrier_id, &building_type).await?;
    Ok(())
}
