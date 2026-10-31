//! The simplified representation drops the proof with the sub attributes.

use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, simplified_matcher::has_value};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateEntity, RepresentationSpelling},
    },
    model::{Entity, Lossless, NgsiLdMediaType, RawJson, Value},
};

/// Pins: An attribute carrying an ngsildproof is retrieved with `options=keyValues` as its value alone, with no
/// seal spelled anywhere
/// Clause: 4.5.4, 4.5.2.2
#[gtest]
#[tokio::test]
async fn n_ent_ret_9027_the_simplified_representation_drops_the_proof_with_the_sub_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_simplified_id = namespace.urn("Building", "simplified")?;
    let building_type = namespace.entity_type("Building")?;
    let building_simplified = Entity::builder().id(building_simplified_id.clone()).entity_type(building_type).build()?;
    // The model refuses it (an attribute proof), so `sealed` is written as text.
    let sealed: RawJson = format!(r#"{{"type": "Property", "value": 21.5, "ngsildproof": {{"type": "Property", "entityIdSealed": "urn:ngsi-ld:Building:{namespace}-simplified", "entityTypeSealed": "Building_{namespace}", "value": {{"type": "DataIntegrityProof", "created": "2025-01-27T21:02:24Z", "verificationMethod": "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT", "cryptosuite": "eddsa-rdfc-2022", "proofPurpose": "assertionMethod", "proofValue": "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS"}}}}}}"#).parse()?;
    let document = UncheckedDocument::from_entity(&building_simplified, Lossless::Normalized).set_member("sealed", &sealed);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_simplified).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&building_simplified_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    expect_that!(String::from_utf8_lossy(&received.body), not(contains_substring("entityIdSealed")));
    expect_that!(String::from_utf8_lossy(&received.body), not(contains_substring("ngsi-ld:entityIdSealed")));
    expect_that!(String::from_utf8_lossy(&received.body), not(contains_substring("ngsi-ld:entityTypeSealed")));
    expect_that!(
        String::from_utf8_lossy(&received.body),
        not(contains_substring("https://uri.etsi.org/ngsi-ld/entityIdSealed"))
    );
    expect_that!(
        String::from_utf8_lossy(&received.body),
        not(contains_substring("https://uri.etsi.org/ngsi-ld/entityTypeSealed"))
    );
    expect_that!(
        String::from_utf8_lossy(&received.body),
        not(contains_substring("default-context/entityIdSealed"))
    );
    expect_that!(
        String::from_utf8_lossy(&received.body),
        not(contains_substring("default-context/entityTypeSealed"))
    );
    let retrieved = &received.outcome;
    let entity = retrieved.body();
    expect_that!(entity, has_value("sealed", Value::Number("21.5".parse()?)));
    Ok(())
}
