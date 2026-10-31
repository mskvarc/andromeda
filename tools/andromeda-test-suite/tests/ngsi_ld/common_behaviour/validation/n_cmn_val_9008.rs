//! A seal outside ngsildproof is refused by merge and accepted inside it.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::MergeEntity},
    model::{Entity, EntityFragment, ErrorType, Lossless, NgsiLdMediaType, RawJson, Value},
};

/// Pins: Merge Entity with `entityIdSealed`, then `entityTypeSealed`, on an ordinary sub-Attribute position of a
/// Property answers 400 `BadRequestData` each time, and the same seals inside the ngsildproof sub-Property answer
/// 204
/// Clause: 4.5.2.2, 5.5.4, 5.6.17.4
#[gtest]
#[tokio::test]
async fn n_cmn_val_9008_a_seal_outside_ngsildproof_is_refused_by_merge_and_accepted_inside_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let fragment = EntityFragment::builder().build()?;
    // The model refuses it (member `entityIdSealed` is not allowed here at temperature), so `temperature` is written as text.
    let temperature: RawJson = format!(r#"{{"type": "Property", "value": 22.0, "entityIdSealed": "urn:ngsi-ld:Building:{namespace}-other"}}"#).parse()?;
    let document = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member("temperature", &temperature);
    let request = broker
        .unchecked(&MergeEntity::builder().id(&building_id).fragment(&fragment).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment_2 = EntityFragment::builder().build()?;
    // The model refuses it (member `entityTypeSealed` is not allowed here at temperature), so `temperature` is written as text.
    let temperature_2: RawJson = format!(r#"{{"type": "Property", "value": 22.0, "entityTypeSealed": "urn:ngsi-ld:Building:{namespace}-other"}}"#).parse()?;
    let document_2 = UncheckedDocument::try_from(fragment_2.render(Lossless::Normalized))?.set_member("temperature", &temperature_2);
    let request_2 = broker
        .unchecked(&MergeEntity::builder().id(&building_id).fragment(&fragment_2).build())?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment_3 = EntityFragment::builder().build()?;
    // The model refuses it (an attribute proof), so `temperature` is written as text.
    let temperature_3: RawJson = format!(r#"{{"type": "Property", "value": 23.0, "ngsildproof": {{"type": "Property", "value": {{"type": "DataIntegrityProof", "cryptosuite": "eddsa-jcs-2022", "proofValue": "z2aB"}}, "entityIdSealed": "urn:ngsi-ld:Building:{namespace}-1", "entityTypeSealed": "Building_{namespace}"}}}}"#).parse()?;
    let document_3 = UncheckedDocument::try_from(fragment_3.render(Lossless::Normalized))?.set_member("temperature", &temperature_3);
    let request_3 = broker
        .unchecked(&MergeEntity::builder().id(&building_id).fragment(&fragment_3).build())?
        .document(&document_3);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("temperature", any_attribute().value(23)));
    Ok(())
}
