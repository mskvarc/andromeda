//! A create body carrying a `GeoProperty` of a million positions is decoded
//! from its text into the typed Entity without a JSON tree of its coordinates
//! (clause 4.7): the extraction -- counting, number check, `@context` reading
//! and decoding -- peaks below 100 MiB above the body it was handed, where the
//! one copy of 1,000,000 positions of 32 bytes the Entity owns is about 32 MB,
//! and it takes well under a second.
//!
//! The counting allocator is this test binary's global allocator, so it
//! measures nothing but this test.

use andromeda_http_extraction::jsonld::JsonLd;
use andromeda_jsonld::user_context_admission::AdmittedContext;
use andromeda_ngsi_ld_model::{
    attribute::instance::{Attribute, AttributeValue},
    entity::Entity,
};
use andromeda_test_allocator::counting_allocator::CountingAllocator;
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
    time::{Duration, Instant},
};
use tower::{Layer, ServiceExt, service_fn};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator::new();

/// The positions of the test polygon's one ring, closing position included.
const POSITIONS: usize = 1_000_000;

/// The extraction may peak this far above what was allocated before it.
const PEAK_BUDGET: usize = 100 * 1024 * 1024;

/// How long the extraction may take in an optimised build; an unoptimised one
/// is allowed ten times that.
const TIME_BUDGET: Duration = Duration::from_secs(1);

/// A create body whose `location` is a Polygon of one ring of [`POSITIONS`]
/// positions, spelled as clients spell coordinates: some with a trailing zero,
/// some integral.
fn polygon_body() -> Result<String, FormatError> {
    let mut body = String::with_capacity(POSITIONS * 24 + 256);
    body.push_str(r#"{"id":"urn:ngsi-ld:Area:1","type":"Area","location":{"type":"GeoProperty","value":{"type":"Polygon","coordinates":[["#);
    for index in 0..POSITIONS {
        let step = if index + 1 == POSITIONS { 0 } else { index };
        if index > 0 {
            body.push(',');
        }
        write!(body, "[13.{:06},52.{:06}0]", step % 1_000_000, (step * 7) % 1_000_000)?;
    }
    body.push_str("]]}}}");
    Ok(body)
}

/// How many positions the `location` geometry of `entity` holds.
fn location_positions(entity: &Entity) -> Option<usize> {
    match entity.location.as_ref()? {
        AttributeValue::Single(instance) => match instance.as_ref() {
            Attribute::GeoProperty(property) => property.value.form().map(|form| form.position_count().get()),
            Attribute::Property(_)
            | Attribute::Relationship(_)
            | Attribute::VocabProperty(_)
            | Attribute::LanguageProperty(_)
            | Attribute::JsonProperty(_)
            | Attribute::ListProperty(_)
            | Attribute::ListRelationship(_) => None,
        },
        AttributeValue::Multiple(_) | AttributeValue::NgsiLdNull => None,
    }
}

#[tokio::test]
async fn a_million_position_polygon_body_decodes_without_a_tree() {
    let body = Bytes::from(polygon_body().expect("the body is written"));
    let length = body.len();
    let request = Request::builder()
        .uri("/ngsi-ld/v1/entities")
        .header(CONTENT_TYPE, "application/json")
        .header(CONTENT_LENGTH, length)
        .body(Body::from(body))
        .expect("the request builds");
    // The broker's stack lifts axum's default body limit and enforces its own
    // published limit instead; a request without one is read in full.
    let extraction = DefaultBodyLimit::disable().layer(service_fn(|request: Request<Body>| async move {
        Ok::<_, Infallible>(JsonLd::<Entity>::from_request(request, &()).await)
    }));
    let mark = ALLOCATOR.mark();
    let started = Instant::now();

    let Ok(extracted) = extraction.oneshot(request).await;
    let decoded = match extracted {
        Ok(JsonLd { body, .. }) => body.admitted(&AdmittedContext::core_only()).await,
        Err(rejection) => panic!("the polygon body is a valid Entity, got {rejection:?}"),
    };

    let elapsed = started.elapsed();
    let peak_above_input = ALLOCATOR.peak_above(mark);
    println!("decoding a body of {length} bytes holding {POSITIONS} positions peaked at {peak_above_input} bytes above the input and took {elapsed:?}");
    let entity = match decoded {
        Ok(entity) => entity,
        Err(refusal) => panic!("the polygon body is a valid Entity, got {refusal:?}"),
    };
    assert_eq!(
        location_positions(&entity),
        Some(POSITIONS),
        "the location is a typed geometry of every position"
    );
    assert!(
        peak_above_input < PEAK_BUDGET,
        "decoding {POSITIONS} positions peaked at {peak_above_input} bytes above the input, over the {PEAK_BUDGET} byte budget"
    );
    let budget = if cfg!(debug_assertions) { TIME_BUDGET * 10 } else { TIME_BUDGET };
    assert!(elapsed < budget, "decoding {POSITIONS} positions took {elapsed:?}, over {budget:?}");
}
