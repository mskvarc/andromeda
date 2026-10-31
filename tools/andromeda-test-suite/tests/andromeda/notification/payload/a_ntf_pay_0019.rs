//! A durable creation notification carries the created entity whole.

use andromeda_test_suite::{
    attribute_matcher::{attribute_of, geo_property, property, relationship},
    broker,
    config::BrokerRole,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    type_matcher::has_type,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    model::{
        AttributeKind,
        Entity,
        EntityId,
        EntityTypeName,
        JsonProperty,
        LanguageMap,
        LanguageProperty,
        Lossless,
        NgsiLdMediaType,
        Property,
        RawJson,
        Relationship,
        Value,
        VocabTerm,
        geometry::{BoundingBox, Geometry, Position, Shape},
    },
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use serde_json::json;

/// Pins: under durable delivery a creation notification carries the created Entity in every member: a Property with
/// `unitCode` and a sub-attribute two levels deep, a `LanguageProperty`, a `GeoProperty` with bbox, a Relationship
/// with `objectType` and a `JsonProperty`
/// Clause: 5.8.6, 4.5
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery (notifications.delivery.mode durable on
/// broker-durable), where the queued event holds the Entity's state as typed fields
#[gtest]
#[tokio::test]
async fn a_ntf_pay_0019_a_durable_creation_notification_carries_the_created_entity_whole() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_durable = broker::client(BrokerRole::Durable)?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = format!("urn:ngsi-ld:Building:{namespace}").parse::<EntityId>()?;
    let owner_id = format!("urn:ngsi-ld:Person:{namespace}").parse::<EntityId>()?;
    let person_type = "Person".parse::<EntityTypeName>()?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker_durable.subscriptions().create(&subscription).send().await?;
    let corner = Position::try_new(13.5, 52.25)?;
    let location = Geometry::from(Shape::Point(corner.clone())).with_bbox(BoundingBox::try_new(corner.clone(), corner)?)?;
    let temperature = Property::builder(Value::Number("20.5".parse()?))
        .unit_code(UnitCode::Cel)
        .property("accuracy", Property::builder(Value::Number("0.5".parse()?)).property("method", "calibrated"));
    let entity = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", temperature)
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Tower"), ("fr", "Tour")])?),
        )
        .geo_property("location", location.clone())
        .relationship("owner", Relationship::builder(owner_id.clone()).object_type(VocabTerm::from(&person_type)))
        .json_property("config", JsonProperty::builder(r#"{"a":[1,2],"b":{"c":null}}"#.parse::<RawJson>()?))
        .build()?;
    broker_durable.entities().create(&entity).send().await?.done()?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(sink.received(1).await?, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), len(eq(1)));
    expect_that!(
        notification.data().as_slice(),
        nth(
            0,
            all!(
                has_id(&building_id),
                has_type(&building_type),
                has_attribute(
                    "temperature",
                    property().value(Value::Number("20.5".parse()?)).unit_code(UnitCode::Cel).sub_attribute(
                        "accuracy",
                        property()
                            .value(Value::Number("0.5".parse()?))
                            .sub_attribute("method", property().value("calibrated"))
                    )
                ),
                has_attribute(
                    "name",
                    attribute_of(AttributeKind::LanguageProperty).language("en", "Tower").language("fr", "Tour")
                ),
                has_attribute("location", geo_property().geometry(location)),
                has_attribute("owner", relationship().object(owner_id).object_type(VocabTerm::from(&person_type))),
                has_attribute("config", attribute_of(AttributeKind::JsonProperty).json(json!({"a": [1, 2], "b": {"c": null}})))
            )
        )
    );
    Ok(())
}
