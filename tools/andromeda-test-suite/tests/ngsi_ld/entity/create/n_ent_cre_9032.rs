//! A hyphenated value type and object type term is created and read back expanded.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property, relationship},
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    object_type::object_type,
    problem::answered,
    wire::content_type,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::CreateEntity},
    model::{APPLICATION_LD_JSON, Entity, NgsiLdMediaType, OneOrMany, VocabTerm},
};

/// The entity as the request writes it.
const BODY: &str = r#"{
    "id": "urn:ngsi-ld:Vehicle:{{ns}}-coerced-types",
    "type": "Vehicle_{{ns}}",
    "reading": {
        "type": "Property",
        "value": "42",
        "valueType": "my-datatype"
    },
    "garage": {
        "type": "Relationship",
        "object": "urn:ngsi-ld:Building:{{ns}}-coerced-types",
        "objectType": "smart-building"
    },
    "garages": {
        "type": "Relationship",
        "object": "urn:ngsi-ld:Building:{{ns}}-coerced-types",
        "objectType": [
            "smart-building",
            "smart-room"
        ]
    },
    "@context": [
        "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
        {
            "my-datatype": "https://example.org/datatype/my-datatype",
            "smart-building": "https://example.org/type/smart-building",
            "smart-room": "https://example.org/type/smart-room"
        }
    ]
}"#;

/// Pins: Create Entity whose @context defines my-datatype, smart-building and smart-room, with a Property
/// `valueType` my-datatype and Relationships `objectType` smart-building and [smart-building, smart-room], answers
/// 201, and Retrieve Entity as JSON-LD reads back each term as the IRI the @context gives it
/// Clause: 4.5.2.2, 4.6.2, 5.2.5, 5.2.6, 5.6.1, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_9032_a_hyphenated_value_type_and_object_type_term_is_created_and_read_back_expanded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_coerced_types_id = namespace.urn("Vehicle", "coerced-types")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder().id(vehicle_coerced_types_id.clone()).entity_type(vehicle_type).build()?;
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
        .retrieve(&vehicle_coerced_types_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "reading",
            property().value_type("https://example.org/datatype/my-datatype".parse::<VocabTerm>()?)
        )
    );
    expect_that!(entity, has_attribute("garage", relationship()));
    expect_that!(
        object_type(entity, "garage"),
        some(eq(&OneOrMany::one("https://example.org/type/smart-building".parse::<VocabTerm>()?)))
    );
    expect_that!(entity, has_attribute("garages", any_attribute()));
    expect_that!(
        object_type(entity, "garages"),
        some(eq(&OneOrMany::try_from(vec![
            "https://example.org/type/smart-building".parse::<VocabTerm>()?,
            "https://example.org/type/smart-room".parse::<VocabTerm>()?,
        ])?))
    );
    Ok(())
}
