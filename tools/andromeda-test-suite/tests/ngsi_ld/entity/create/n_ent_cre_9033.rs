//! A hyphenated vocab term is created and read back expanded.

use andromeda_test_suite::{
    attribute_matcher::attribute_of,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    problem::answered,
    wire::content_type,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::CreateEntity},
    model::{APPLICATION_LD_JSON, AttributeKind, Entity, NgsiLdMediaType, OneOrMany, VocabTerm},
};

/// The entity as the request writes it.
const BODY: &str = r#"{
    "id": "urn:ngsi-ld:Vehicle:{{ns}}-vocab-term",
    "type": "Vehicle_{{ns}}",
    "category": {
        "type": "VocabProperty",
        "vocab": "non-commercial"
    },
    "categories": {
        "type": "VocabProperty",
        "vocab": [
            "non-commercial",
            "commercial"
        ]
    },
    "@context": [
        "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
        {
            "non-commercial": "https://example.org/vocab/non-commercial",
            "commercial": "https://example.org/vocab/commercial"
        }
    ]
}"#;

/// Pins: Create Entity whose @context defines non-commercial and commercial, with `VocabProperty` category vocab
/// non-commercial and categories vocab [non-commercial, commercial], answers 201, and Retrieve Entity as JSON-LD
/// reads back each term as the IRI the @context gives it
/// Clause: 4.5.2.2, 4.6.2, 5.2.35, 5.6.1, 5.7.1; C.2.2
#[gtest]
#[tokio::test]
async fn n_ent_cre_9033_a_hyphenated_vocab_term_is_created_and_read_back_expanded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_vocab_term_id = namespace.urn("Vehicle", "vocab-term")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder().id(vehicle_vocab_term_id.clone()).entity_type(vehicle_type).build()?;
    // The model refuses the hyphenated terms as names, so the body is sent as written.
    let request = content_type(
        broker
            .unchecked(&CreateEntity::builder().entity(&vehicle).build())?
            .body(Bytes::from(BODY.replace("{{ns}}", namespace.token()))),
        &APPLICATION_LD_JSON,
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&vehicle_vocab_term_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "category",
            attribute_of(AttributeKind::VocabProperty).vocab(OneOrMany::one("https://example.org/vocab/non-commercial".parse::<VocabTerm>()?))
        )
    );
    expect_that!(
        entity,
        has_attribute(
            "categories",
            attribute_of(AttributeKind::VocabProperty).vocab(OneOrMany::try_from(vec![
                "https://example.org/vocab/non-commercial".parse::<VocabTerm>()?,
                "https://example.org/vocab/commercial".parse::<VocabTerm>()?,
            ])?)
        )
    );
    Ok(())
}
