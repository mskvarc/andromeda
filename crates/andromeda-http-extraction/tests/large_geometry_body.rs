//! A create body carrying a `GeoProperty` of a million positions is decoded
//! from its text into the typed Entity without a JSON tree of its coordinates
//! (clause 4.7): the extraction -- counting, number check, `@context` reading
//! and decoding -- peaks below 100 MiB above the body it was handed, where the
//! one copy of 1,000,000 positions of 32 bytes the Entity owns is about 32 MB,
//! and its time grows linearly with the positions.
//!
//! The measurement counts the allocations of the calling thread alone, so the
//! other tests of the binary do not disturb it; the extraction runs on a
//! current-thread runtime for that reason.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
use andromeda_http_extraction::jsonld::JsonLd;
use andromeda_jsonld::user_context_admission::AdmittedContext;
use andromeda_ngsi_ld_model::{
    attribute::instance::{Attribute, AttributeValue},
    entity::Entity,
};
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
use tokio::runtime::{Builder, Runtime};
use tower::{Layer, ServiceExt, service_fn};

/// The positions of the large polygon's one ring, closing position included.
const POSITIONS: usize = 1_000_000;

/// The positions of the small polygon the large one's time is compared with.
const BASELINE_POSITIONS: usize = POSITIONS / 4;

/// The extraction may peak this far above what was allocated before it.
const PEAK_BUDGET: u64 = 100 * 1024 * 1024;

/// How many times longer the large polygon may take than the small one: four
/// times the positions take about four times as long when decoding is linear
/// and sixteen times when it is quadratic.
const MAX_TIME_RATIO: f64 = 8.0;

/// What decoding one body cost.
struct Decoding {
    /// The bytes of the body.
    length: usize,
    /// The most bytes allocated above what was allocated before the extraction.
    peak_above_input: u64,
    /// The wall-clock time of the extraction.
    elapsed: Duration,
}

/// A create body whose `location` is a Polygon of one ring of `positions`
/// positions, spelled as clients spell coordinates: some with a trailing zero,
/// some integral.
fn polygon_body(positions: usize) -> Result<String, FormatError> {
    let mut body = String::with_capacity(positions * 24 + 256);
    body.push_str(r#"{"id":"urn:ngsi-ld:Area:1","type":"Area","location":{"type":"GeoProperty","value":{"type":"Polygon","coordinates":[["#);
    for index in 0..positions {
        let step = if index + 1 == positions { 0 } else { index };
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

/// Extracts a create body of a polygon of `positions` positions into a typed
/// Entity on `runtime`, checking that its geometry keeps every position.
fn decode(runtime: &Runtime, positions: usize) -> Decoding {
    let body = Bytes::from(polygon_body(positions).expect("the body is written"));
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
    let started = Instant::now();
    let mut decoded = None;

    let measured = allocation_counter::measure(|| {
        decoded = Some(runtime.block_on(async {
            let Ok(extracted) = extraction.oneshot(request).await;
            match extracted {
                Ok(JsonLd { body, .. }) => body.admitted(&AdmittedContext::core_only()).await,
                Err(rejection) => panic!("the polygon body is a valid Entity, got {rejection:?}"),
            }
        }));
    });

    let elapsed = started.elapsed();
    let entity = match decoded.expect("the extraction ran") {
        Ok(entity) => entity,
        Err(refusal) => panic!("the polygon body is a valid Entity, got {refusal:?}"),
    };
    assert_eq!(
        location_positions(&entity),
        Some(positions),
        "the location is a typed geometry of every position"
    );
    Decoding {
        length,
        peak_above_input: measured.bytes_max,
        elapsed,
    }
}

#[test]
fn a_million_position_polygon_body_decodes_without_a_tree() {
    let runtime = Builder::new_current_thread().enable_all().build().expect("the runtime builds");

    let baseline = decode(&runtime, BASELINE_POSITIONS);
    let large = decode(&runtime, POSITIONS);

    let Decoding {
        length,
        peak_above_input,
        elapsed,
    } = large;
    let ratio = elapsed.as_secs_f64() / baseline.elapsed.as_secs_f64();
    println!(
        "decoding a body of {length} bytes holding {POSITIONS} positions peaked at {peak_above_input} bytes above the input and took {elapsed:?}, {ratio:.1} times the {:?} of {BASELINE_POSITIONS} positions",
        baseline.elapsed
    );
    assert!(
        peak_above_input < PEAK_BUDGET,
        "decoding {POSITIONS} positions peaked at {peak_above_input} bytes above the input, over the {PEAK_BUDGET} byte budget"
    );
    assert!(
        ratio < MAX_TIME_RATIO,
        "decoding {POSITIONS} positions took {ratio:.1} times as long as {BASELINE_POSITIONS}, over {MAX_TIME_RATIO}"
    );
}
