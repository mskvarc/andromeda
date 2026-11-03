//! An entity spelled under a set alias of `@type` and a nest alias is created as the entity spelled without them.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    problem::answered,
    wire::content_type,
};
use bytes::Bytes;
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::CreateEntity},
    model::{APPLICATION_LD_JSON, Entity, NgsiLdMediaType, OneOrMany},
};

/// The entity as the request writes it, the shape JSON-LD 1.1 compaction gives it under its `@context`.
const BODY: &str = r#"{
    "id": "urn:ngsi-ld:Building:{{ns}}-contained",
    "tps": ["Building_{{ns}}"],
    "n": {
        "height": {
            "tps": ["Property"],
            "value": 12,
            "unitCode": "MTR"
        }
    },
    "@context": {
        "tps": {"@id": "@type", "@container": "@set"},
        "n": "@nest"
    }
}"#;

/// Pins: Create Entity under an inline @context defining tps as an alias of @type with the @set container and n as
/// an alias of @nest, writing the type and the attribute type as arrays of one and the attribute under n, answers
/// 201, and the Entity reads back as the Entity written without the containers
/// Clause: 5.5.4, 5.5.7, 5.6.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_9062_an_entity_spelled_under_a_set_alias_of_type_and_a_nest_alias_is_created() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_contained_id = namespace.urn("Building", "contained")?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(building_contained_id.clone()).entity_type(building_type.clone()).build()?;
    // The model has no place for members written under containers, so the body is sent as written.
    let request = content_type(
        broker
            .unchecked(&CreateEntity::builder().entity(&building).build())?
            .body(Bytes::from(BODY.replace("{{ns}}", namespace.token()))),
        &APPLICATION_LD_JSON,
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_contained_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_contained_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type)));
    expect_that!(entity, has_attribute("height", property().value(12).unit_code(UnitCode::Mtr)));
    expect_that!(entity, lacks_attribute("n"));
    expect_that!(entity, lacks_attribute("tps"));
    Ok(())
}
