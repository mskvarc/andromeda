//! A periodic notification is amended too.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, attribute_of, property, relationship},
    attribute_member::Member,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    problem::answered,
    type_matcher::{has_type, type_count},
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        operation::{CreateEntity, CreateSubscription},
    },
    model::{
        AttributeKind,
        Entity,
        JsonProperty,
        LanguageMap,
        LanguageProperty,
        ListProperty,
        Lossless,
        NgsiLdMediaType,
        OneOrMany,
        Property,
        RawJson,
        Relationship,
        Value,
        VocabProperty,
        VocabTerm,
    },
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription with `timeInterval` 1 and `ngsildConformance` 1.0 is notified periodically with the first
/// type alone, no scope, the `LanguageProperty` as a Property, no `unitCode` and the `ListRelationship` as a
/// Relationship, where the periodic control keeps them
/// Clause: 4.3.6.8, Table 5.2.12-1, 5.8.6
///
/// Each amended Subscription has a control Subscription that declares no `ngsildConformance` and is otherwise the
/// same, so every amendment asserted is a difference the control does not show.
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9036_a_periodic_notification_is_amended_too() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_periodic_id = namespace.urn("Vehicle", "periodic")?;
    let building_periodic_id = namespace.urn("Building", "periodic")?;
    let subscription_periodic_amended_id = namespace.subscription_id("periodic-amended")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_periodic_control_id = namespace.subscription_id("periodic-control")?;
    let vehicle_periodic = Entity::builder()
        .id(vehicle_periodic_id.clone())
        .entity_type(vec![namespace.entity_type("Vehicle")?, namespace.entity_type("Car")?])
        .scope(["/conformance"])
        .expires_at("2099-01-01T00:00:00Z".parse()?)
        .property(
            "speed",
            Property::builder(20)
                .unit_code(UnitCode::Cel)
                .value_type("Number".parse()?)
                .observed_at("2025-01-01T00:00:00Z".parse()?),
        )
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Tower"), ("de", "Turm")])?),
        )
        .vocab_property("category", VocabProperty::builder("commercial".parse::<VocabTerm>()?))
        .json_property("config", JsonProperty::builder(r#"{"level":3}"#.parse::<RawJson>()?))
        .list_property("tags", ListProperty::builder(vec![Value::from("a"), Value::from("b")]))
        .relationship(
            "refBuilding",
            Relationship::builder(building_periodic_id)
                .object_type("Building".parse::<VocabTerm>()?)
                .observed_at("2025-01-01T00:00:00Z".parse()?),
        )
        .property("temperature", Property::builder(25).dataset_id(namespace.dataset_id("Dataset", "one")?))
        .property("temperature", 30)
        .build()?;
    // The model refuses it (the model reads it as written otherwise [at $.refRooms.objectList[0] (written "urn:ngsi-ld:Room:zqns-periodic")]: {"id":"urn:ngsi-ld:Vehicle:zqns-periodic","type":["Vehicle_zqns","Car_zqns"],"scope":["/conformance"],"expiresAt":"2099-01-01T00:00:00Z","speed":{"type":"Property","value":20,"valueType":"Number","unitCode":"CEL","observedAt":"2025-01-01T00:00:00Z"},"name":{"type":"LanguageProperty","languageMap":{"en":"Tower","de":"Turm"}},"category":{"type":"VocabProperty","vocab":"commercial"},"config":{"type":"JsonProperty","json":{"level":3}},"tags":{"type":"ListProperty","valueList":["a","b"]},"refRooms":{"type":"ListRelationship","objectList":[{"object":"urn:ngsi-ld:Room:zqns-periodic"},{"object":"urn:ngsi-ld:Room:zqns-periodic-second"}]},"refBuilding":{"type":"Relationship","objectType":"Building","object":"urn:ngsi-ld:Building:zqns-periodic","observedAt":"2025-01-01T00:00:00Z"},"temperature":[{"type":"Property","value":25,"datasetId":"urn:ngsi-ld:Dataset:zqns-one"},{"type":"Property","value":30}]}), so `refRooms` is written as text.
    let ref_rooms: RawJson =
        format!(r#"{{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:{namespace}-periodic", "urn:ngsi-ld:Room:{namespace}-periodic-second"]}}"#)
            .parse()?;
    let document = UncheckedDocument::from_entity(&vehicle_periodic, Lossless::Normalized).set_member("refRooms", &ref_rooms);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&vehicle_periodic).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let subscription_periodic_amended = Subscription::builder()
        .id(subscription_periodic_amended_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_periodic_id.clone())])
        .time_interval(1)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("periodic-amended")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a subscription ngsildConformance), so `ngsildConformance` is written as text.
    let ngsild_conformance: RawJson = r#""1.0""#.parse()?;
    let document_2 = UncheckedDocument::try_from(subscription_periodic_amended.render())?.set_member("ngsildConformance", &ngsild_conformance);
    let request_2 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_periodic_amended,
        })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let subscription_periodic_control = Subscription::builder()
        .id(subscription_periodic_control_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_periodic_id.clone())])
        .time_interval(1)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("periodic-control")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_periodic_control).send().await?;
    sink.wait_for_at_least("periodic-control", 1).await?;
    sink.wait_for_at_least("periodic-amended", 1).await?;
    let notifications = sink.received("periodic-control").await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification.data().as_slice(),
        nth(0, all!(has_attribute("name", attribute_of(AttributeKind::LanguageProperty)), type_count(2)))
    );
    let notifications_2 = sink.received("periodic-amended").await?;
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    let item = notification_2.data().first().or_fail()?;
    // Table 4.3.6.8-3 types the `object` of a Relationship as String or String[], so every target is kept in list order.
    let rooms = OneOrMany::try_from(vec![namespace.urn("Room", "periodic")?, namespace.urn("Room", "periodic-second")?])?;
    expect_that!(item.scope.as_ref(), none());
    expect_that!(
        notification_2.data().as_slice(),
        nth(
            0,
            all!(
                has_attribute("name", property()),
                has_attribute("speed", any_attribute().without(Member::UnitCode)),
                has_attribute("refRooms", relationship().object(rooms)),
                has_type(&vehicle_type)
            )
        )
    );
    broker.subscriptions().delete(&subscription_periodic_amended_id).send().await?;
    broker.subscriptions().delete(&subscription_periodic_control_id).send().await?;
    Ok(())
}
