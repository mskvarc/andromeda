//! Every write endpoint refuses a body sent as `application/xml` with 415 (clause 6.3.4).

use crate::{building_payloads::building_simple_attributes, vehicle_evolution_payloads::VEHICLE_SPEED_FUEL_TERMS};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::{api_uri, content_type},
};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriBuf;
use mediatype::{MediaType, media_type};
use ngsi_ld::{
    http::{
        ContextDelivery,
        operation::{BatchCreate, CreateEntity, CreateOrUpdateTemporalEvolution, CreateSubscription, PartialAttributeUpdate, UpdateSubscription},
    },
    model::{Attribute, AttributeName, DatasetId, NgsiLdMediaType, Property, Representation, evolution::TemporalEntity},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment, SubscriptionId},
};

/// The media type every case sends its body as.
const XML: MediaType<'static> = media_type!(APPLICATION / XML);

/// The client of `broker` sending `vocabulary` in the body, as the tests send the test vocabulary.
fn in_body(broker: &BrokerClient, vocabulary: &Vocabulary) -> Result<BrokerClient> {
    Ok(broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody))
}

/// Pins: Verify throwing 415 HTTP status code (Unsupported Media Type) if "Content-Type" header is not
/// "application/json" or "application/ld+json" (patch /entities/{entityId}/attrs/{attrId})
/// Clause: 6.3.4
#[gtest]
#[tokio::test]
async fn e048_01_01_patch_entity_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["speed"]).await?;
    let client = in_body(&broker, &vocabulary)?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let patch = Attribute::from(Property::builder("BMW").build()?).into_patch().into();
    let update = PartialAttributeUpdate::builder().id(&id).name(&speed).attribute(&patch).build();
    let answer = client.send_unchecked(content_type(client.unchecked(&update)?, &XML)?).await;
    expect_that!(&answer, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE));
    broker.entities().delete(&id).send().await.any_answer();
    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Verify throwing 415 HTTP status code (Unsupported Media Type) if "Content-Type" header is not
/// "application/json" or "application/ld+json" (patch /subscriptions/{subscriptionId})
/// Clause: 6.3.4
/// Vocabulary: none (the test references the core @context only)
#[gtest]
#[tokio::test]
async fn e048_01_02_patch_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let client = broker
        .with_context("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld".parse::<IriBuf>()?)?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let id = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint = Endpoint::builder(format!("http://notifications.invalid/notify/{namespace}/2"))
        .accept(NgsiLdMediaType::Json)
        .build()?;
    let fragment = SubscriptionFragment::builder()
        .notification(NotificationParams::builder(endpoint).format(Representation::Simplified).build()?)
        .build()?;
    let update = UpdateSubscription { id: &id, fragment: &fragment };
    let answer = client.send_unchecked(content_type(client.unchecked(&update)?, &XML)?).await;
    expect_that!(&answer, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE));
    broker.subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Verify throwing 415 HTTP status code (Unsupported Media Type) if "Content-Type" header is not
/// "application/json" or "application/ld+json" (post /entities/)
/// Clause: 6.3.4
#[gtest]
#[tokio::test]
async fn e048_01_03_post_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name", "subCategory", "airQualityLevel", "almostFull"]).await?;
    let client = in_body(&broker, &vocabulary)?;
    let entity = building_simple_attributes(&namespace, 1)?;
    let create = CreateEntity::builder().entity(&entity).build();
    let request = content_type(client.unchecked(&create)?, &XML)?.uri(api_uri(&client, "entities/")?);
    expect_that!(&client.send_unchecked(request).await, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE));
    broker.entities().delete(&entity.id).send().await.any_answer();
    Ok(())
}

/// Pins: Verify throwing 415 HTTP status code (Unsupported Media Type) if "Content-Type" header is not
/// "application/json" or "application/ld+json" (post /subscriptions/)
/// Clause: 6.3.4
#[gtest]
#[tokio::test]
async fn e048_01_04_post_subscriptions() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let client = in_body(&broker, &vocabulary)?;
    let endpoint = Endpoint::builder(format!("http://notifications.invalid/notify/{namespace}/1"))
        .accept(NgsiLdMediaType::Json)
        .build()?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    // The subscription owns its identifier, and its fields are private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities(["Building"])
        .notification(NotificationParams::builder(endpoint).format(Representation::Simplified).build()?)
        .build()?;
    let _subscription = Cleanup::of(&broker).subscription_of(&subscription);
    let create = CreateSubscription { subscription: &subscription };
    let request = content_type(client.unchecked(&create)?, &XML)?.uri(api_uri(&client, "subscriptions/")?);
    expect_that!(&client.send_unchecked(request).await, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE));
    broker.subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Verify throwing 415 HTTP status code (Unsupported Media Type) if "Content-Type" header is not
/// "application/json" or "application/ld+json" (post /entityOperations/create)
/// Clause: 6.3.4
#[gtest]
#[tokio::test]
async fn e048_01_05_post_batch_create() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name", "subCategory", "airQualityLevel", "almostFull"]).await?;
    let client = in_body(&broker, &vocabulary)?;
    let entities = [building_simple_attributes(&namespace, 1)?, building_simple_attributes(&namespace, 2)?];
    let create = BatchCreate::builder().entities(&entities).build();
    expect_that!(
        &client.send_unchecked(content_type(client.unchecked(&create)?, &XML)?).await,
        answered(StatusCode::UNSUPPORTED_MEDIA_TYPE)
    );
    let ids = entities.map(|entity| entity.id);
    broker.entities().batch_delete(&ids).send().await.any_answer();
    Ok(())
}

/// Pins: Verify throwing 415 HTTP status code (Unsupported Media Type) if "Content-Type" header is not
/// "application/json" or "application/ld+json" (post /temporal/entities/)
/// Clause: 6.3.4
#[gtest]
#[tokio::test]
async fn e048_01_06_post_temporal_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &VEHICLE_SPEED_FUEL_TERMS).await?;
    let client = in_body(&broker, &vocabulary)?;
    let evolution = TemporalEntity::builder()
        .id(namespace.urn("Vehicle", 1)?)
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
    let request = content_type(client.unchecked(&create)?, &XML)?.uri(api_uri(&client, "temporal/entities/")?);
    expect_that!(&client.send_unchecked(request).await, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE));
    broker.temporal().delete(&evolution.id).send().await.any_answer();
    Ok(())
}
