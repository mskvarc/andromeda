//! An entity spelled under user aliases of core terms is created.

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

/// The entity as the request writes it.
const BODY: &str = r#"{
    "ident": "urn:ngsi-ld:Building:{{ns}}-aliased",
    "kind": "Building_{{ns}}",
    "height": {
        "kind": "Property",
        "v": 12,
        "unitCode": "MTR"
    },
    "@context": {
        "kind": "@type",
        "ident": "@id",
        "v": "https://uri.etsi.org/ngsi-ld/hasValue"
    }
}"#;

/// Pins: Create Entity under an inline @context aliasing @id as ident, @type as kind and ngsi-ld:hasValue as v
/// answers 201, and the Entity reads back as the plain spelling writes it
/// Clause: 4.4, 5.5.7, 5.6.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_9046_an_entity_spelled_under_user_aliases_of_core_terms_is_created() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_aliased_id = namespace.urn("Building", "aliased")?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(building_aliased_id.clone()).entity_type(building_type.clone()).build()?;
    // The model has no place for members spelled under aliases of core terms, so the body is sent as written.
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
        .retrieve(&building_aliased_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_aliased_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type)));
    expect_that!(
        entity,
        has_attribute("height", property().value(12).unit_code(UnitCode::Mtr).without_sub_attribute("v"))
    );
    expect_that!(entity, lacks_attribute("kind"));
    Ok(())
}
