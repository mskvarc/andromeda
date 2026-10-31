//! Expired attributes are removed while the others and the entity expiry stand.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    eventually::retried,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{
        DateTime,
        Entity,
        GeoProperty,
        NgsiLdMediaType,
        Property,
        geometry::{Position, Shape},
    },
    query::geoquery,
};

/// Pins: a Property and a location whose `expiresAt` has passed are removed from the Entity, which keeps its other
/// Attribute and its own future `expiresAt`, and a geo-query at the removed location no longer matches it while it
/// matches an Entity whose location stands there
/// Clause: 4.22, 4.10, 6.3.11, 5.7.1.4, 5.7.2.4
/// Recorded: 4.22 lets an expired attribute "be automatically removed" by a periodic clean-up whose lag the spec
/// leaves open, and does not refuse an `expiresAt` already past; Andromeda accepts it and removes the attribute,
/// and the file waits up to 60 seconds for that
#[gtest]
#[tokio::test]
async fn o_ent_ret_0044_expired_attributes_are_removed_while_the_others_and_the_entity_expiry_stand() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_expiring_id = namespace.urn("Vehicle", "expiring")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_standing_id = namespace.urn("Vehicle", "standing")?;
    let entities = [
        Entity::builder()
            .id(vehicle_expiring_id.clone())
            .entity_type(vehicle_type.clone())
            .expires_at("2099-12-31T23:59:59Z".parse()?)
            .property("speed", Property::builder(80).expires_at("2020-01-01T00:00:00Z".parse()?))
            .geo_property(
                "location",
                GeoProperty::builder(Shape::Point(Position::try_new(13.0, 52.0)?)).expires_at("2020-01-01T00:00:00Z".parse()?),
            )
            .property("fuel", 55)
            .build()?,
        Entity::builder()
            .id(vehicle_standing_id.clone())
            .entity_type(vehicle_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(13.0, 52.0)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    retried(async || {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&vehicle_expiring_id)
            .system_attributes(SystemAttributes::Include)
            .send()
            .await?;
        let entity = retrieved.body();
        verify_that!(entity, lacks_attribute("speed"))?;
        verify_that!(entity, lacks_attribute("location"))?;
        verify_that!(entity.expires_at, some(eq("2099-12-31T23:59:59Z".parse::<DateTime>()?)))?;
        verify_that!(entity, has_attribute("fuel", any_attribute().value(55)))?;
        Ok(())
    })
    .await?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(1000, Shape::Point(Position::try_new(13.0, 52.0)?)))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_standing_id)));
    Ok(())
}
