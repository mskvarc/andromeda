//! A refused ring or line is named with its rule and position.

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
        operation::{CreateEntity, CreateOrUpdateTemporalEvolution, CreateSubscription, RegisterContextSource},
    },
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson, evolution::TemporalEntity},
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: The `BadRequestData` refusing an open ring, a ring of three positions, an open hole or a `LineString` of
/// one position names the RFC 7946 rule broken, the attribute, and where in coordinates the ring or line sits, on a
/// Core create, a temporal create, the three query surfaces, the query body, a subscription `geoQ` and a
/// registration location
/// Clause: 4.7.2, 4.10, 5.5.4
/// Recorded: clause 5.5.4 fixes the `BadRequestData` type and no detail; the wording is Andromeda's
#[gtest]
#[tokio::test]
async fn a_cmn_val_9020_a_refused_ring_or_line_is_named_with_its_rule_and_position() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let ring_probe_open_ring_detail_id = namespace.urn("RingProbe", "open-ring-detail")?;
    let ring_probe_type = namespace.entity_type("RingProbe")?;
    let ring_probe_short_ring_detail_id = namespace.urn("RingProbe", "short-ring-detail")?;
    let ring_probe_open_hole_detail_id = namespace.urn("RingProbe", "open-hole-detail")?;
    let ring_probe_short_line_detail_id = namespace.urn("RingProbe", "short-line-detail")?;
    let ring_probe_temporal_id = namespace.urn("RingProbe", "temporal")?;
    let subscription_open_ring_id = namespace.subscription_id("open-ring")?;
    let registration_open_ring_id = namespace.registration_id("open-ring")?;
    let ring_probe_open_ring_detail = Entity::builder()
        .id(ring_probe_open_ring_detail_id)
        .entity_type(ring_probe_type.clone())
        .build()?;
    // The model refuses it (invalid geometry at location), so `location` is written as text.
    let location: RawJson = r#"{"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1]]]}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&ring_probe_open_ring_detail, Lossless::Normalized).set_member("location", &location);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&ring_probe_open_ring_detail).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("GeoJSON linear ring is open; RFC 7946 section 3.1.6 requires its last position to repeat its first")
            .with_detail_containing("(at coordinates[0])")
            .with_detail_containing("location")
    );
    let ring_probe_short_ring_detail = Entity::builder()
        .id(ring_probe_short_ring_detail_id)
        .entity_type(ring_probe_type.clone())
        .build()?;
    // The model refuses it (invalid geometry at observationSpace), so `observationSpace` is written as text.
    let observation_space: RawJson = r#"{"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [0, 0]]]}}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&ring_probe_short_ring_detail, Lossless::Normalized).set_member("observationSpace", &observation_space);
    let request_2 = broker
        .unchecked(&CreateEntity::builder().entity(&ring_probe_short_ring_detail).build())?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("GeoJSON linear ring has 3 of the four or more positions RFC 7946 section 3.1.6 requires")
            .with_detail_containing("(at coordinates[0])")
            .with_detail_containing("observationSpace")
    );
    let ring_probe_open_hole_detail = Entity::builder()
        .id(ring_probe_open_hole_detail_id)
        .entity_type(ring_probe_type.clone())
        .build()?;
    // The model refuses it (invalid geometry at operationSpace), so `operationSpace` is written as text.
    let operation_space: RawJson = r#"{"type": "GeoProperty", "value": {"type": "MultiPolygon", "coordinates": [[[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]], [[[10, 10], [20, 10], [20, 20], [10, 20], [10, 10]], [[12, 12], [14, 12], [14, 14], [12, 14]]]]}}"#.parse()?;
    let document_3 = UncheckedDocument::from_entity(&ring_probe_open_hole_detail, Lossless::Normalized).set_member("operationSpace", &operation_space);
    let request_3 = broker
        .unchecked(&CreateEntity::builder().entity(&ring_probe_open_hole_detail).build())?
        .document(&document_3);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(
        &answer_3,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("GeoJSON linear ring is open; RFC 7946 section 3.1.6 requires its last position to repeat its first")
            .with_detail_containing("(at coordinates[1][1])")
            .with_detail_containing("operationSpace")
    );
    let ring_probe_short_line_detail = Entity::builder()
        .id(ring_probe_short_line_detail_id)
        .entity_type(ring_probe_type.clone())
        .build()?;
    // The model refuses it (invalid geometry at location), so `location` is written as text.
    let location_2: RawJson = r#"{"type": "GeoProperty", "value": {"type": "LineString", "coordinates": [[0, 0]]}}"#.parse()?;
    let document_4 = UncheckedDocument::from_entity(&ring_probe_short_line_detail, Lossless::Normalized).set_member("location", &location_2);
    let request_4 = broker
        .unchecked(&CreateEntity::builder().entity(&ring_probe_short_line_detail).build())?
        .document(&document_4);
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(
        &answer_4,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("GeoJSON LineString has 1 of the two or more positions RFC 7946 section 3.1.4 requires")
            .with_detail_containing("(at coordinates)")
            .with_detail_containing("location")
    );
    let ring_probe_temporal_evolution = TemporalEntity::builder()
        .id(ring_probe_temporal_id)
        .entity_type(ring_probe_type.clone())
        .build()?;
    // The model refuses it (invalid geometry at location), so `location` is written as text.
    let location_3: RawJson = r#"[{"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]]}, "observedAt": "2025-01-01T00:00:00Z"}, {"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1]]]}, "observedAt": "2025-01-01T00:01:00Z"}]"#.parse()?;
    let document_5 = UncheckedDocument::try_from(ring_probe_temporal_evolution.render(Lossless::Normalized))?.set_member("location", &location_3);
    let request_5 = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&ring_probe_temporal_evolution).build())?
        .document(&document_5);
    let answer_5 = broker.send_unchecked(request_5).await;
    expect_that!(
        &answer_5,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("GeoJSON linear ring is open; RFC 7946 section 3.1.6 requires its last position to repeat its first")
    );
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_6 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!(
            "entities?type=RingProbe_{namespace}&georel=within&geometry=Polygon&coordinates=%5B%5B%5B0%2C0%5D%2C%5B1%2C0%5D%2C%5B1%2C1%5D%2C%5B0%2C1%5D%5D%5D"
        ),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_6 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_6).await;
    expect_that!(
        &answer_6,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("GeoJSON linear ring is open; RFC 7946 section 3.1.6 requires its last position to repeat its first")
    );
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_7 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("temporal/entities?type=RingProbe_{namespace}&timerel=after&timeAt=2020-01-01T00%3A00%3A00Z&georel=within&geometry=Polygon&coordinates=%5B%5B%5B0%2C0%5D%2C%5B1%2C0%5D%2C%5B1%2C1%5D%2C%5B0%2C1%5D%5D%5D"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_7 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_7).await;
    expect_that!(
        &answer_7,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("GeoJSON linear ring is open; RFC 7946 section 3.1.6 requires its last position to repeat its first")
    );
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_8 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("csourceRegistrations?type=RingProbe_{namespace}&georel=within&geometry=Polygon&coordinates=%5B%5B%5B0%2C0%5D%2C%5B1%2C0%5D%2C%5B1%2C1%5D%2C%5B0%2C1%5D%5D%5D"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_8 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_8).await;
    expect_that!(
        &answer_8,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("GeoJSON linear ring is open; RFC 7946 section 3.1.6 requires its last position to repeat its first")
    );
    // The model refuses a member of this document, so it is sent as written.
    let request_9 = request_to(&broker, Method::POST, "entityOperations/query")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"type\": \"Query\",\n    \"entities\": [\n        {{\n            \"type\": \"RingProbe_{namespace}\"\n        }}\n    ],\n    \"geoQ\": {{\n        \"georel\": \"within\",\n        \"geometry\": \"Polygon\",\n        \"coordinates\": [\n            [\n                [\n                    0,\n                    0\n                ],\n                [\n                    1,\n                    0\n                ],\n                [\n                    1,\n                    1\n                ],\n                [\n                    0,\n                    1\n                ]\n            ]\n        ]\n    }}\n}}")));
    let answer_9 = broker.send_unchecked(request_9).await;
    expect_that!(
        &answer_9,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("GeoJSON linear ring is open; RFC 7946 section 3.1.6 requires its last position to repeat its first")
    );
    let subscription_open_ring = Subscription::builder()
        .id(subscription_open_ring_id)
        .entities([ring_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid geoquery at geoQ), so `geoQ` is written as text.
    let geo_q: RawJson = r#"{"georel": "within", "geometry": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1]]]}"#.parse()?;
    let document_6 = UncheckedDocument::try_from(subscription_open_ring.render())?.set_member("geoQ", &geo_q);
    let request_10 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_open_ring,
        })?
        .document(&document_6);
    let answer_10 = broker.send_unchecked(request_10).await;
    expect_that!(
        &answer_10,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("GeoJSON linear ring is open; RFC 7946 section 3.1.6 requires its last position to repeat its first")
    );
    let registration_open_ring = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_open_ring_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(ring_probe_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .build()?;
    // The model refuses it (invalid geometry at location), so `location` is written as text.
    let location_4: RawJson = r#"{"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1]]]}"#.parse()?;
    let document_7 = UncheckedDocument::try_from(registration_open_ring.render())?.set_member("location", &location_4);
    let request_11 = broker
        .unchecked(&RegisterContextSource {
            registration: &registration_open_ring,
        })?
        .document(&document_7);
    let answer_11 = broker.send_unchecked(request_11).await;
    expect_that!(
        &answer_11,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("GeoJSON linear ring is open; RFC 7946 section 3.1.6 requires its last position to repeat its first")
    );
    Ok(())
}
