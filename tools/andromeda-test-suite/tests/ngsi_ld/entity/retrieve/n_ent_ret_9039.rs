//! A read under a context aliasing at type answers type.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, Entity, NgsiLdMediaType, OneOrMany},
};

/// Pins: Retrieve Entity under a Link @context aliasing @type as kind and `ngsi-ld:hasValue` as v answers with type
/// and value, never with the aliases
/// Clause: 4.4, 4.5.0, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_ret_9039_a_read_under_a_context_aliasing_at_type_answers_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let building_read_id = namespace.urn("Building", "read")?;
    let building_type = namespace.entity_type("Building")?;
    let linked = broker.with_context(Context::Single(contexts.reference("contexts/aliasing.jsonld")?))?;
    contexts
        .serve(
            "contexts/aliasing.jsonld",
            r#"{
      "@context": {
        "kind": "@type",
        "v": "https://uri.etsi.org/ngsi-ld/hasValue"
      }
    }"#,
            Some("application/ld+json"),
        )
        .await?;
    let building_read = Entity::builder()
        .id(building_read_id.clone())
        .entity_type(building_type.clone())
        .property("height", 12)
        .build()?;
    broker.entities().create(&building_read).send().await?.done()?;
    let retrieved = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_read_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_read_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(entity, lacks_attribute("kind"));
    expect_that!(
        entity,
        has_attribute("height", property().value(12).without_sub_attribute("kind").without_sub_attribute("v"))
    );
    Ok(())
}
