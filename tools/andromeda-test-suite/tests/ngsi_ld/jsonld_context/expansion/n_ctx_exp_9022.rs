//! The core contexts scoped proof term still expands.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    core_context::core_context_list,
    entity_matcher::has_attribute,
    namespace::Namespace,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity with an ngsildproof sub-Property carrying `entityIdSealed` and `entityTypeSealed` under the
/// Core @context answers 201, and the proof reads back with its seal and its value
/// Clause: 4.5.2.2, 5.5.7, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9022_the_core_contexts_scoped_proof_term_still_expands() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let core_context_list_body = broker.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).build()?;
    // The model refuses it (an attribute proof), so `speed` is written as text.
    let speed: RawJson = format!(r#"{{"type": "Property", "value": 1, "ngsildproof": {{"type": "Property", "value": {{"type": "DataIntegrityProof", "cryptosuite": "eddsa-jcs-2022"}}, "entityIdSealed": "urn:ngsi-ld:Device:{namespace}-1", "entityTypeSealed": "Device_{namespace}"}}}}"#).parse()?;
    let document = UncheckedDocument::from_entity(&device, Lossless::Normalized).set_member("speed", &speed);
    let request = core_context_list_body
        .unchecked(&CreateEntity::builder().entity(&device).build())?
        .document(&document);
    let answer = core_context_list_body.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "speed",
            any_attribute().sealed_to(device_id.clone()).proof_member(["cryptosuite"], "eddsa-jcs-2022")
        )
    );
    Ok(())
}
