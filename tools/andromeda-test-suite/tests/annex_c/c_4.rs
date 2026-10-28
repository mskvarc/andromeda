//! Annex C.4: the subscription the annex prints, its `geoQ` reference position on the WGS 84 axes or, as printed,
//! off them (clauses 4.10 and 5.8.1).

use crate::{
    served_context::{ServedContexts, linked, ok_body},
    vehicle_example::aggregated_context,
};
use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateSubscription},
    model::{
        ErrorType,
        Input,
        NgsiLdMediaType,
        RawJson,
        Representation,
        geometry::{Position, Shape},
    },
    query::{EntitySelector, geoquery, q},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};
use serde_json::json;

/// The id of the example's subscription.
fn subscription_id(namespace: &Namespace) -> Result<SubscriptionId> {
    Ok(namespace.urn_text("Subscription", "mySubscription").parse()?)
}

/// The example's subscription to fast Vehicles near the point at longitude 100 and latitude -1.
fn subscription(namespace: &Namespace) -> Result<Subscription<Input>> {
    let endpoint = Endpoint::builder("http://my.endpoint.org/notify").accept(NgsiLdMediaType::Json).build()?;
    Ok(Subscription::builder()
        .id(subscription_id(namespace)?)
        .entities([EntitySelector::new(namespace.suffixed("Vehicle").parse()?)])
        .watched_attributes(["speed"])
        .q(q::attr("speed").gt(50).build()?)
        .geo_q(geoquery::near_max(2000, Shape::Point(Position::try_new(100.0, -1.0)?)).build()?)
        .notification(
            NotificationParams::builder(endpoint)
                .attributes(["speed"])
                .format(Representation::Simplified)
                .build()?,
        )
        .build()?)
}

/// The example's subscription as the annex writes it: `format` spelled `keyValues`, which the builder writes as
/// `simplified`, and the `geoQ` reference written as `coordinates`.
fn printed_document(subscription: &Subscription<Input>, coordinates: [i64; 2]) -> Result<UncheckedDocument> {
    let notification: RawJson = json!({
        "attributes": ["speed"],
        "format": "keyValues",
        "endpoint": {"uri": "http://my.endpoint.org/notify", "accept": "application/json"},
    })
    .to_string()
    .parse()?;
    let geo_q: RawJson = json!({"georel": "near;maxDistance==2000", "geometry": "Point", "coordinates": coordinates})
        .to_string()
        .parse()?;
    Ok(UncheckedDocument::try_from(subscription.render())?
        .set_member("geoQ", &geo_q)
        .set_member("notification", &notification))
}

/// Pins: The subscription C.4 prints, with its geoQ reference position written on the WGS 84 axes, is created and
/// retrieved with every member it prints; members 5.2.12 defaults or reports are not compared
/// Clause: C.4, 4.10, 5.2.12, 5.8.1, 5.8.3
/// Corrected: geoq-latitude: the geoQ reference [-1, 100] is replayed with its ordinates exchanged, latitude 100
/// being off the WGS 84 axes (4.10)
#[gtest]
#[tokio::test]
async fn c_4_0001_the_subscription_round_trips_with_an_in_range_reference() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    let subscription = subscription(&namespace)?;
    let _subscription = Cleanup::of(&broker).subscription_of(&subscription);
    let request = broker
        .unchecked(&CreateSubscription { subscription: &subscription })?
        .document(&printed_document(&subscription, [100, -1])?);
    let created = broker.send_unchecked(request).await;
    expect_that!(&created, answered(StatusCode::CREATED));
    let id = subscription_id(&namespace)?;
    let body = ok_body(
        broker
            .with_accept(NgsiLdMediaType::JsonLd)
            .with_responses()
            .subscriptions()
            .retrieve(&id)
            .send()
            .await,
    )?;
    expect_that!(body.get("id"), some(eq(&json!(id.as_str()))));
    expect_that!(body.get("type"), some(eq(&json!("Subscription"))));
    expect_that!(body.get("entities"), some(eq(&json!([{"type": namespace.suffixed("Vehicle")}]))));
    expect_that!(body.get("watchedAttributes"), some(eq(&json!(["speed"]))));
    expect_that!(body.get("q"), some(eq(&json!("speed>50"))));
    expect_that!(
        body.get("geoQ"),
        some(eq(&json!({"georel": "near;maxDistance==2000", "geometry": "Point", "coordinates": [100, -1]})))
    );
    expect_that!(body.pointer("/notification/attributes"), some(eq(&json!(["speed"]))));
    expect_that!(body.pointer("/notification/format"), some(eq(&json!("keyValues"))));
    expect_that!(body.pointer("/notification/endpoint/uri"), some(eq(&json!("http://my.endpoint.org/notify"))));
    expect_that!(body.pointer("/notification/endpoint/accept"), some(eq(&json!("application/json"))));
    broker.subscriptions().delete(&id).send().await?;
    served.stop();
    Ok(())
}

/// Pins: The subscription exactly as C.4 prints it names a reference position with latitude 100, off the WGS 84
/// axes, so it is refused as `BadRequestData` and not created
/// Clause: C.4, 4.10, 5.8.1, 6.3.2
#[gtest]
#[tokio::test]
async fn c_4_0002_the_printed_reference_position_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    let subscription = subscription(&namespace)?;
    // The builder refuses latitude 100, so the printed reference is written into the document.
    let _subscription = Cleanup::of(&broker).subscription_of(&subscription);
    let request = broker
        .unchecked(&CreateSubscription { subscription: &subscription })?
        .document(&printed_document(&subscription, [-1, 100])?);
    let refused = broker.send_unchecked(request).await;
    expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .subscriptions()
        .retrieve(&subscription_id(&namespace)?)
        .send()
        .await;
    expect_that!(&retrieved, answered(StatusCode::NOT_FOUND));
    served.stop();
    Ok(())
}
