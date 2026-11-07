//! A create body carrying a `GeoProperty` geometry past the position limit
//! costs the broker a bounded amount of memory whatever its size (clause
//! 6.3.2): the decoding records the geometry's coordinates no further than a
//! geometry within the limit can hold, so a body four times as large peaks as
//! high as a small one.
//!
//! The measurement counts the allocations of the calling thread alone, so the
//! other tests of the binary do not disturb it; the extraction runs on a
//! current-thread runtime for that reason.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "clippy reaches only `#[test]` functions in a member crate's integration test, and these helpers fail the test by panicking"
)]
use andromeda_http_extraction::{jsonld::JsonLd, request_decode_bounds::RequestDecodeBounds};
use andromeda_jsonld::user_context_admission::AdmittedContext;
use andromeda_ngsi_ld_geometry::geometry_census::GeometryPositionLimit;
use andromeda_ngsi_ld_model::{
    attribute::{
        geo_property_value::GeoPropertyValue,
        instance::{Attribute, AttributeValue},
    },
    entity::Entity,
};
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, FromRequest},
    http::{
        Request,
        header::{CONTENT_LENGTH, CONTENT_TYPE},
    },
};
use std::{
    convert::Infallible,
    fmt::{Error as FormatError, Write},
    num::NonZeroUsize,
};
use tokio::runtime::{Builder, Runtime};
use tower::{Layer, ServiceExt, service_fn};

/// The positions the geometry may hold.
const LIMIT: usize = 100_000;

/// The positions of the smaller body, over the limit.
const SMALL_POSITIONS: usize = 400_000;

/// The positions of the larger body, four times the smaller one.
const LARGE_POSITIONS: usize = 1_600_000;

/// Refusing a geometry may peak this far above what was allocated before the
/// extraction: the coordinates of three limits of ordinates, with room.
const PEAK_BUDGET: u64 = 32 * 1024 * 1024;

/// How much higher the larger body may peak than the smaller one.
const MAX_PEAK_RATIO: f64 = 1.25;

/// A create body whose `location` is a Polygon of one ring of `positions`
/// positions.
fn polygon_body(positions: usize) -> Result<String, FormatError> {
    let mut body = String::with_capacity(positions * 24 + 256);
    body.push_str(r#"{"id":"urn:ngsi-ld:Area:1","type":"Area","location":{"type":"GeoProperty","value":{"type":"Polygon","coordinates":[["#);
    for index in 0..positions {
        let step = if index + 1 == positions { 0 } else { index };
        if index > 0 {
            body.push(',');
        }
        write!(body, "[13.{:06},52.{:06}]", step % 1_000_000, (step * 7) % 1_000_000)?;
    }
    body.push_str("]]}}}");
    Ok(body)
}

/// Whether the `location` of `entity` is a geometry read past the limit.
fn location_is_oversized(entity: &Entity) -> bool {
    match entity.location.as_ref() {
        Some(AttributeValue::Single(instance)) => match instance.as_ref() {
            Attribute::GeoProperty(property) => matches!(property.value, GeoPropertyValue::Oversized(_)),
            Attribute::Property(_)
            | Attribute::Relationship(_)
            | Attribute::VocabProperty(_)
            | Attribute::LanguageProperty(_)
            | Attribute::JsonProperty(_)
            | Attribute::ListProperty(_)
            | Attribute::ListRelationship(_) => false,
        },
        Some(AttributeValue::Multiple(_) | AttributeValue::NgsiLdNull) | None => false,
    }
}

/// Extract a create body of a polygon of `positions` positions on `runtime`
/// under the position limit, and the most bytes the extraction allocated above
/// what was allocated before it.
fn peak_of_extracting(runtime: &Runtime, positions: usize) -> u64 {
    let body = Bytes::from(polygon_body(positions).expect("the body is written"));
    let mut request = Request::builder()
        .uri("/ngsi-ld/v1/entities")
        .header(CONTENT_TYPE, "application/json")
        .header(CONTENT_LENGTH, body.len())
        .body(Body::from(body))
        .expect("the request builds");
    request.extensions_mut().insert(RequestDecodeBounds::new(
        GeometryPositionLimit::new(NonZeroUsize::new(LIMIT).expect("a positive limit")),
        JsonValueLimit::new(NonZeroUsize::new(1_000_000).expect("a positive limit")),
    ));
    let extraction = DefaultBodyLimit::disable().layer(service_fn(|request: Request<Body>| async move {
        Ok::<_, Infallible>(JsonLd::<Entity>::from_request(request, &()).await)
    }));
    let mut decoded = None;

    let measured = allocation_counter::measure(|| {
        decoded = Some(runtime.block_on(async {
            let Ok(extracted) = extraction.oneshot(request).await;
            match extracted {
                Ok(JsonLd { body, .. }) => body.admitted(&AdmittedContext::core_only()).await,
                Err(rejection) => panic!("the body is a valid Entity, got {rejection:?}"),
            }
        }));
    });

    let entity = match decoded.expect("the extraction ran") {
        Ok(entity) => entity,
        Err(refusal) => panic!("the body decodes, its geometry read as oversized, got {refusal:?}"),
    };
    assert!(
        location_is_oversized(&entity),
        "a geometry of {positions} positions is read as past the {LIMIT} position limit"
    );
    measured.bytes_max
}

#[test]
fn a_geometry_past_the_position_limit_costs_the_same_whatever_its_size() {
    let runtime = Builder::new_current_thread().enable_all().build().expect("the runtime builds");

    let small = peak_of_extracting(&runtime, SMALL_POSITIONS);
    let large = peak_of_extracting(&runtime, LARGE_POSITIONS);

    println!("extracting {SMALL_POSITIONS} positions peaked at {small} bytes, {LARGE_POSITIONS} positions at {large} bytes");
    assert!(
        large < PEAK_BUDGET,
        "extracting {LARGE_POSITIONS} positions peaked at {large} bytes, over the {PEAK_BUDGET} byte budget"
    );
    #[expect(clippy::cast_precision_loss, reason = "byte counts far below 2^52")]
    let ratio = large as f64 / small as f64;
    assert!(
        ratio < MAX_PEAK_RATIO,
        "extracting {LARGE_POSITIONS} positions peaked {ratio:.2} times as high as {SMALL_POSITIONS}, over {MAX_PEAK_RATIO}"
    );
}
