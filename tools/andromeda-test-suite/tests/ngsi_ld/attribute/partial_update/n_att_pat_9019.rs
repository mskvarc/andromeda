//! A partial update of ngsildproof keeps the seals bare.

use andromeda_test_suite::{
    bare_seals::bare_seals,
    body_json::json_equals,
    broker,
    namespace::Namespace,
    problem::answered,
    wire::request_to,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateEntity},
    model::{Entity, Lossless, RawJson},
};

/// Pins: Partial Attribute Update of an attribute named ngsildproof with new seals answers 204 and the attribute
/// reads back with the new seals as bare strings
/// Clause: 4.5.2.2, 5.6.4
#[gtest]
#[tokio::test]
async fn n_att_pat_9019_a_partial_update_of_ngsildproof_keeps_the_seals_bare() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_partial_update_id = namespace.urn("Building", "partial-update")?;
    let building_type = namespace.entity_type("Building")?;
    let building_partial_update = Entity::builder().id(building_partial_update_id.clone()).entity_type(building_type).build()?;
    // The model refuses it (an attribute proof), so `ngsildproof` is written as text.
    let ngsildproof: RawJson = format!(r#"{{"type": "Property", "entityIdSealed": "urn:ngsi-ld:Building:{namespace}-partial-update", "entityTypeSealed": "Building_{namespace}", "value": {{"type": "DataIntegrityProof", "created": "2025-01-27T21:02:24Z", "verificationMethod": "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT", "cryptosuite": "eddsa-rdfc-2022", "proofPurpose": "assertionMethod", "proofValue": "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS"}}}}"#).parse()?;
    let document = UncheckedDocument::from_entity(&building_partial_update, Lossless::Normalized).set_member("ngsildproof", &ngsildproof);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_partial_update).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // The model refuses the seals as members of an attribute, so the patch is sent as written.
    let request_2 = json_request_to(
        &broker,
        Method::PATCH,
        &format!("entities/{building_partial_update_id}/attrs/ngsildproof"),
        &format!(
            r#"{{"type": "Property", "entityIdSealed": "urn:ngsi-ld:Building:{namespace}-resealed", "entityTypeSealed": "Building_{namespace}", "value": {{"type": "DataIntegrityProof", "created": "2025-01-27T21:02:24Z", "verificationMethod": "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT", "cryptosuite": "eddsa-rdfc-2022", "proofPurpose": "assertionMethod", "proofValue": "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS"}}}}"#
        ),
    )?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::NO_CONTENT));
    // The model reads no seal as a member of an attribute, so the answer is read as received.
    let request_3 = request_to(&broker, Method::GET, &format!("entities/{building_partial_update_id}"))?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, answered(StatusCode::OK));
    let received = answer_3?;
    let body = received.body();
    expect_that!(
        body,
        json_equals("$.ngsildproof.entityIdSealed", format!("urn:ngsi-ld:Building:{namespace}-resealed"))
    );
    expect_that!(body, json_equals("$.ngsildproof.entityTypeSealed", format!("Building_{namespace}")));
    expect_that!(body, json_equals("$.ngsildproof.value.type", "DataIntegrityProof"));
    expect_that!(body, json_equals("$.ngsildproof.value.created", "2025-01-27T21:02:24Z"));
    expect_that!(
        body,
        json_equals(
            "$.ngsildproof.value.verificationMethod",
            "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT"
        )
    );
    expect_that!(body, json_equals("$.ngsildproof.value.cryptosuite", "eddsa-rdfc-2022"));
    expect_that!(body, json_equals("$.ngsildproof.value.proofPurpose", "assertionMethod"));
    expect_that!(
        body,
        json_equals("$.ngsildproof.value.proofValue", "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS")
    );
    expect_that!(body, bare_seals());
    Ok(())
}
