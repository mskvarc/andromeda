//! A read under a type map container answers every instance the map holds.

use andromeda_test_suite::{
    attribute_matcher::{geo_property, property},
    broker,
    entity_matcher::{has_instance, has_instances},
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Context,
        Entity,
        GeoProperty,
        NgsiLdMediaType,
        OneOrMany,
        geometry::{Position, Shape},
    },
};

/// Pins: Retrieve Entity whose `speed` has a Property instance and a `GeoProperty` instance, under a Link @context
/// defining `speed` with the @type container, answers both instances in the NGSI-LD shape although compaction
/// writes `speed` as one object keyed by the two types
/// Clause: 4.5.5, 5.5.7, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_ret_9040_a_read_under_a_type_map_container_answers_every_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let vehicle_mapped_id = namespace.urn("Vehicle", "mapped")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let plain = broker.with_context(Context::Single(contexts.reference("contexts/plain.jsonld")?))?;
    let mapped = broker.with_context(Context::Single(contexts.reference("contexts/mapped.jsonld")?))?;
    contexts
        .serve(
            "contexts/plain.jsonld",
            &format!(r#"{{"@context": {{"speed": "https://test.andromeda/{namespace}/speed"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "contexts/mapped.jsonld",
            &format!(r#"{{"@context": {{"speed": {{"@id": "https://test.andromeda/{namespace}/speed", "@container": "@type"}}}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    let vehicle_mapped = Entity::builder()
        .id(vehicle_mapped_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .geo_property(
            "speed",
            GeoProperty::builder(Shape::Point(Position::try_new(2.2945, 48.8584)?)).dataset_id(namespace.dataset_id("Dataset", "geo")?),
        )
        .build()?;
    plain.entities().create(&vehicle_mapped).send().await?.done()?;
    let retrieved = mapped
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_mapped_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&vehicle_mapped_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(vehicle_type)));
    expect_that!(entity, has_instances("speed", 2));
    expect_that!(entity, has_instance("speed", property().value(50)));
    expect_that!(entity, has_instance("speed", geo_property()));
    Ok(())
}
