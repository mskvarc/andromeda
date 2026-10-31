//! Entity type sealed compacts to its term.

use andromeda_test_suite::{
    body_json::{json_absent, json_exists},
    broker,
    namespace::Namespace,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateEntity, RepresentationSpelling, RetrieveEntity},
    },
    model::{Entity, Lossless, NgsiLdMediaType, RawJson},
};
use std::str;

/// Pins: An ngsildproof is retrieved normalized and concise with an `entityTypeSealed` member and no
/// `ngsi-ld:entityTypeSealed` member or other prefixed seal spelling
/// Clause: 4.4, 4.5.2.2
#[gtest]
#[tokio::test]
async fn n_ent_ret_9026_entity_type_sealed_compacts_to_its_term() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_term_id = namespace.urn("Building", "term")?;
    let building_type = namespace.entity_type("Building")?;
    let building_term = Entity::builder().id(building_term_id.clone()).entity_type(building_type.clone()).build()?;
    let sealed: RawJson = format!(
        r#"{{"type": "Property", "value": 21.5, "ngsildproof": {{"type": "Property", "entityIdSealed": "{building_term_id}", "entityTypeSealed": "{building_type}", "value": {{"type": "DataIntegrityProof", "created": "2025-01-27T21:02:24Z", "verificationMethod": "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT", "cryptosuite": "eddsa-rdfc-2022", "proofPurpose": "assertionMethod", "proofValue": "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS"}}}}}}"#
    )
    .parse()?;
    // The model refuses the `ngsildproof` member, so `sealed` is written as text beside the entity.
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_term).build())?
        .document(&UncheckedDocument::from_entity(&building_term, Lossless::Normalized).set_member("sealed", &sealed));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    // The entity reader refuses `ngsildproof` and the spelling is the subject, so the answer is read as received.
    let request_2 = json.unchecked(&RetrieveEntity::builder().id(&building_term_id).format(Lossless::Normalized).build())?;
    let retrieved_2 = broker.send_unchecked(request_2).await?;
    let body_2 = retrieved_2.body();
    expect_that!(body_2, json_exists("$.sealed.ngsildproof.entityTypeSealed"));
    expect_that!(body_2, json_absent("$.sealed.ngsildproof['ngsi-ld:entityTypeSealed']"));
    let text_2 = str::from_utf8(body_2)?;
    for spelling in [
        "ngsi-ld:entityIdSealed",
        "ngsi-ld:entityTypeSealed",
        "https://uri.etsi.org/ngsi-ld/entityIdSealed",
        "https://uri.etsi.org/ngsi-ld/entityTypeSealed",
        "default-context/entityIdSealed",
        "default-context/entityTypeSealed",
    ] {
        expect_that!(text_2, not(contains_substring(spelling)));
    }
    // The entity reader refuses `ngsildproof` and the spelling is the subject, so the answer is read as received.
    let request_3 = json.unchecked(
        &RetrieveEntity::builder()
            .id(&building_term_id)
            .format(Lossless::Concise)
            .spelling(RepresentationSpelling::default().in_options())
            .build(),
    )?;
    let retrieved_3 = broker.send_unchecked(request_3).await?;
    let body_3 = retrieved_3.body();
    expect_that!(body_3, json_exists("$.sealed.ngsildproof.entityTypeSealed"));
    expect_that!(body_3, json_absent("$.sealed.ngsildproof['ngsi-ld:entityTypeSealed']"));
    let text_3 = str::from_utf8(body_3)?;
    for spelling in [
        "ngsi-ld:entityIdSealed",
        "ngsi-ld:entityTypeSealed",
        "https://uri.etsi.org/ngsi-ld/entityIdSealed",
        "https://uri.etsi.org/ngsi-ld/entityTypeSealed",
        "default-context/entityIdSealed",
        "default-context/entityTypeSealed",
    ] {
        expect_that!(text_3, not(contains_substring(spelling)));
    }
    Ok(())
}
