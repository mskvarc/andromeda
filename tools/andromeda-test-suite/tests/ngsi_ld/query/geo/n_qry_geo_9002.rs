//! An open reference ring is refused on every surface.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE},
};
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateSubscription, RegisterContextSource},
    },
    model::{ErrorType, NgsiLdMediaType, RawJson},
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A geoquery whose reference Polygon has an open ring answers 400 `BadRequestData` on Query Entities, Query
/// Temporal Evolution of Entities, Query Context Source Registrations and the Query Entities POST binding, and
/// Create Subscription with that `geoQ` and Create Context Source Registration with that location are refused alike
/// Clause: 4.10, 5.7.2.4, 5.7.4.4, 5.10.2.4, 5.8.1.4, 5.9.2.4
#[gtest]
#[tokio::test]
async fn n_qry_geo_9002_an_open_reference_ring_is_refused_on_every_surface() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_open_ring_id = namespace.subscription_id("open-ring")?;
    let ring_probe_type = namespace.entity_type("RingProbe")?;
    let registration_open_ring_id = namespace.registration_id("open-ring")?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!(
            "entities?type=RingProbe_{namespace}&georel=within&geometry=Polygon&coordinates=%5B%5B%5B0%2C0%5D%2C%5B1%2C0%5D%2C%5B1%2C1%5D%2C%5B0%2C1%5D%5D%5D"
        ),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("temporal/entities?type=RingProbe_{namespace}&timerel=after&timeAt=2020-01-01T00%3A00%3A00Z&georel=within&geometry=Polygon&coordinates=%5B%5B%5B0%2C0%5D%2C%5B1%2C0%5D%2C%5B1%2C1%5D%2C%5B0%2C1%5D%5D%5D"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_3 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("csourceRegistrations?type=RingProbe_{namespace}&georel=within&geometry=Polygon&coordinates=%5B%5B%5B0%2C0%5D%2C%5B1%2C0%5D%2C%5B1%2C1%5D%2C%5B0%2C1%5D%5D%5D"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The model refuses a member of this document, so it is sent as written.
    let request_4 = request_to(&broker, Method::POST, "entityOperations/query")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"type\": \"Query\",\n    \"entities\": [\n        {{\n            \"type\": \"RingProbe_{namespace}\"\n        }}\n    ],\n    \"geoQ\": {{\n        \"georel\": \"within\",\n        \"geometry\": \"Polygon\",\n        \"coordinates\": [\n            [\n                [\n                    0,\n                    0\n                ],\n                [\n                    1,\n                    0\n                ],\n                [\n                    1,\n                    1\n                ],\n                [\n                    0,\n                    1\n                ]\n            ]\n        ]\n    }}\n}}")));
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_open_ring = Subscription::builder()
        .id(subscription_open_ring_id)
        .entities([ring_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid geoquery at geoQ), so `geoQ` is written as text.
    let geo_q: RawJson = r#"{"georel": "within", "geometry": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1]]]}"#.parse()?;
    let document = UncheckedDocument::try_from(subscription_open_ring.render())?.set_member("geoQ", &geo_q);
    let request_5 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_open_ring,
        })?
        .document(&document);
    let answer_5 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let registration_open_ring = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_open_ring_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(ring_probe_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .build()?;
    // The model refuses it (invalid geometry at location), so `location` is written as text.
    let location: RawJson = r#"{"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1]]]}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(registration_open_ring.render())?.set_member("location", &location);
    let request_6 = broker
        .unchecked(&RegisterContextSource {
            registration: &registration_open_ring,
        })?
        .document(&document_2);
    let answer_6 = broker.send_unchecked(request_6).await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
