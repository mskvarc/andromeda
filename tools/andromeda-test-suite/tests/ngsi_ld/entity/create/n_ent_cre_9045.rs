//! An entity and attribute spelled with at id and at type are created.

use andromeda_test_suite::{
    attribute_matcher::{property, relationship},
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    problem::answered,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::CreateEntity},
    model::{Entity, NgsiLdMediaType, OneOrMany},
};

/// The entity as the request writes it.
const BODY: &str = r#"{
    "@id": "urn:ngsi-ld:Building:{{ns}}-keywords",
    "@type": "Building_{{ns}}",
    "height": {
        "@type": "Property",
        "value": 12,
        "observedBy": {
            "@type": "Relationship",
            "object": "urn:ngsi-ld:Device:{{ns}}-1"
        }
    }
}"#;

/// Pins: Create Entity whose id, type and attribute types are written as @id and @type answers 201, and the Entity
/// reads back with id, type and value
/// Clause: 4.4, 5.6.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_9045_an_entity_and_attribute_spelled_with_at_id_and_at_type_are_created() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_keywords_id = namespace.urn("Building", "keywords")?;
    let building_type = namespace.entity_type("Building")?;
    let device_id = namespace.urn("Device", 1)?;
    let building = Entity::builder().id(building_keywords_id.clone()).entity_type(building_type.clone()).build()?;
    // The model refuses `@id` and `@type` as member names, so the body is sent as written.
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building).build())?
        .body(Bytes::from(BODY.replace("{{ns}}", namespace.token())));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_keywords_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_keywords_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type)));
    expect_that!(
        entity,
        has_attribute("height", property().value(12).sub_attribute("observedBy", relationship().object(device_id)))
    );
    expect_that!(entity, lacks_attribute("@id"));
    expect_that!(entity, lacks_attribute("@type"));
    Ok(())
}
