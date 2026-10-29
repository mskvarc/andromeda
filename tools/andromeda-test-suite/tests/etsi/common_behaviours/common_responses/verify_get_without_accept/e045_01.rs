//! ETSI `045_01`: a GET whose `Accept` field is `*/*` is answered as `application/json`, with a `Link` field
//! (clause 6.3.4).

use crate::suite_payloads::{VEHICLE_SPEED_FUEL_TERMS, building_simple_attributes};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    config::SuiteConfig,
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::{content_type, header_value, header_values, send_to},
};
use googletest::prelude::*;
use http::{
    HeaderMap,
    StatusCode,
    header::{CONTENT_TYPE, LINK},
};
use ngsi_ld::{
    http::{
        ContextDelivery,
        headers::Accept,
        operation::{CreateEntity, CreateOrUpdateTemporalEvolution, CreateSubscription, RetrieveSubscription},
    },
    model::{APPLICATION_JSON, DatasetId, EntityTypeName, NgsiLdMediaType, Property, Representation, evolution::TemporalEntity},
    query::{IdPattern, temporal_query},
    registration::{CSourceRegistration, EntityInfo, RegistrationId, RegistrationInfo},
    subscription::{Endpoint, EntitySubscription, NotificationParams, Subscription, SubscriptionId},
};

/// The client of `broker` sending `vocabulary` in the body, as the create cases send the suite's `@context`.
fn in_body(broker: &BrokerClient, vocabulary: &Vocabulary) -> Result<BrokerClient> {
    Ok(broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody))
}

/// The client of `broker` for the GET cases: the `@context` in the `Link` field and `Accept: */*`.
fn reading(broker: &BrokerClient, vocabulary: &Vocabulary) -> Result<BrokerClient> {
    Ok(broker.with_context(vocabulary.context())?.with_accept(Accept::any()))
}

/// The answer names `application/json` as its Content-Type and carries a `Link` field whose first value is not empty.
fn expect_json_with_link(headers: &HeaderMap) {
    expect_that!(header_value(headers, &CONTENT_TYPE), some(eq("application/json")));
    let links = header_values(headers, &LINK);
    expect_that!(links.first().is_some_and(|link| !link.is_empty()), is_true());
}

/// Pins: Verify that on a GET HTTP request if nothing is specified on the Accept header, "application/json" is assumed
/// (/entities/{entityId})
/// Clause: 6.3.4
#[gtest]
#[tokio::test]
async fn e045_01_01_retrieve_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name", "subCategory", "airQualityLevel", "almostFull"]).await?;
    let entity = building_simple_attributes(&namespace, 1)?;
    let create = CreateEntity::builder().entity(&entity).build();
    let created = send_to(&in_body(&broker, &vocabulary)?, &create, "entities/").await;
    verify_that!(&created, answered(StatusCode::CREATED))?;

    let retrieved = reading(&broker, &vocabulary)?.with_heads().entities().retrieve(&entity.id).send().await;
    verify_that!(&retrieved, answered(StatusCode::OK))?;
    expect_json_with_link(retrieved?.head.headers());

    broker.entities().delete(&entity.id).send().await.any_answer();
    broker.temporal().delete(&entity.id).send().await.any_answer();
    Ok(())
}

/// Pins: Verify that on a GET HTTP request if nothing is specified on the Accept header, "application/json" is assumed
/// (/subscriptions/{subscriptionId})
/// Clause: 6.3.4
#[gtest]
#[tokio::test]
async fn e045_01_02_retrieve_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint = Endpoint::builder(format!("http://notifications.invalid/notify/{namespace}/1"))
        .accept(NgsiLdMediaType::Json)
        .build()?;
    let subscription = Subscription::builder()
        .id(id.clone())
        .time_interval(5)
        .entities(["Building"])
        .notification(NotificationParams::builder(endpoint).format(Representation::Simplified).build()?)
        .build()?;
    let _subscription = Cleanup::of(&broker).subscription_of(&subscription);
    let create = CreateSubscription { subscription: &subscription };
    let created = send_to(&in_body(&broker, &vocabulary)?, &create, "subscriptions/").await;
    verify_that!(&created, answered(StatusCode::CREATED))?;

    let reader = reading(&broker, &vocabulary)?;
    let retrieve = RetrieveSubscription::builder().id(&id).kind(EntitySubscription).build();
    let request = content_type(reader.unchecked(&retrieve)?, &APPLICATION_JSON)?;
    let retrieved = reader.send_unchecked(request).await;
    verify_that!(&retrieved, answered(StatusCode::OK))?;
    expect_json_with_link(retrieved?.headers());

    broker.subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Verify that on a GET HTTP request if nothing is specified on the Accept header, "application/json" is assumed
/// (/csourceRegistrations/)
/// Clause: 6.3.4
/// Changed: the suite sets every id member ($..id) to the registration id, so the registered entity id is the
/// registration id and the query's id parameter matches it; the port keeps that
/// Changed: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it matches
/// only the test's own entities.
#[gtest]
#[tokio::test]
async fn e045_01_03_query_csource_registrations() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(
        &broker,
        &namespace,
        &[
            "Vehicle",
            "brandName",
            "speed",
            "isParked",
            "OffStreetParking",
            "availableSpotNumber",
            "totalSpotNumber",
            "isNextToBuilding",
        ],
    )
    .await?;
    let config = SuiteConfig::get()?;
    let id: RegistrationId = namespace.urn_text("ContextSourceRegistration", 1).parse()?;
    let vehicle = namespace.urn("ContextSourceRegistration", 1)?;
    let downtown: IdPattern = format!("urn:ngsi-ld:OffStreetParking:{}-.*downtown$", namespace.token()).parse()?;
    let forty_seven: IdPattern = format!("urn:ngsi-ld:OffStreetParking:{}-.*47$", namespace.token()).parse()?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(id.clone())
        .information([
            RegistrationInfo::builder()
                .entities([EntityInfo::new("Vehicle".parse::<EntityTypeName>()?).with_id(vehicle.clone())])
                .property_names(["brandName", "speed"])
                .relationship_names(["isParked"])
                .build()?,
            RegistrationInfo::builder()
                .entities([
                    EntityInfo::new("OffStreetParking".parse::<EntityTypeName>()?).with_id_pattern(downtown),
                    EntityInfo::new("OffStreetParking".parse::<EntityTypeName>()?).with_id_pattern(forty_seven),
                ])
                .property_names(["availableSpotNumber", "totalSpotNumber"])
                .relationship_names(["isNextToBuilding"])
                .build()?,
        ])
        .expires_at("2035-11-29T14:53:15Z".parse()?)
        .build()?;
    let _registration = Cleanup::of(&broker).registration_of(&registration);
    in_body(&broker, &vocabulary)?.registrations().register(&registration).send().await?;

    let found = reading(&broker, &vocabulary)?
        .with_heads()
        .registrations()
        .query()
        .entity_type("Vehicle")
        .ids(vec![vehicle])
        .send()
        .await;
    verify_that!(&found, answered(StatusCode::OK))?;
    expect_json_with_link(found?.head.headers());

    expect_that!(broker.registrations().delete(&id).send().await, ok(anything()));
    Ok(())
}

/// Pins: Verify that on a GET HTTP request if nothing is specified on the Accept header, "application/json" is assumed
/// (/temporal/entities)
/// Clause: 6.3.4
#[gtest]
#[tokio::test]
async fn e045_01_04_query_temporal_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &VEHICLE_SPEED_FUEL_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let evolution = TemporalEntity::builder()
        .id(id.clone())
        .entity_type("Vehicle")
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(80).observed_at("2020-09-01T12:05:00Z".parse()?))
        .property("fuelLevel", Property::builder(67).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(53).observed_at("2020-09-01T13:05:00Z".parse()?))
        .property(
            "fuelLevel",
            Property::builder(40)
                .observed_at("2020-09-01T14:07:00Z".parse()?)
                .dataset_id(DatasetId::from(namespace.urn("Vehicle", "12345-fuel")?.as_uri().clone())),
        )
        .build()?;
    let create = CreateOrUpdateTemporalEvolution::builder().entity(&evolution).build();
    let created = send_to(&in_body(&broker, &vocabulary)?, &create, "temporal/entities/").await;
    verify_that!(&created, answered(StatusCode::CREATED))?;

    let found = reading(&broker, &vocabulary)?
        .with_heads()
        .temporal()
        .query(temporal_query::after("2020-08-01T12:05:00Z".parse()?))
        .entity_type("Vehicle")
        .send()
        .await;
    verify_that!(&found, answered(StatusCode::OK))?;
    expect_json_with_link(found?.head.headers());

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}
