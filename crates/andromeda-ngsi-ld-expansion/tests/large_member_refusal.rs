//! A `Relationship` `object`, a `ListRelationship` `objectList`, a
//! `VocabProperty` `vocab`, a `LanguageProperty` `languageMap` and an entity `scope` of millions
//! of entries cost the broker a bounded amount of memory (clause 6.3.2).
//!
//! Each body stays under the default request value limit of 4,000,000 and is
//! far over the entity value limit of 1,000,000. The body is decoded within the
//! entity value limit, which stops recording a member at the first value past
//! it, so decoding peaks well under what the whole member would cost. The walk
//! bounded by the entity limit then counts the member by the census rule before
//! it serializes and expands it, so the refusal peaks below 512 MiB above the
//! entity it reads.
//!
//! The measurement counts the allocations of the calling thread alone, so the
//! other tests of the binary do not disturb it.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "clippy reaches only `#[test]` functions in a member crate's integration test, and these helpers fail the test by panicking"
)]
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_expansion::{error::ExpansionError, normalization::Normalization, typed_expander::TypedExpander};
use andromeda_ngsi_ld_geometry::geometry_census::GeometryPositionLimit;
use andromeda_ngsi_ld_model::{
    attribute::instance::AttributeValue,
    entity::Entity,
    request_body::{DecodeBounds, RequestBody},
};
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use andromeda_ngsi_ld_representation::{compact_node::context_reading::ContextReading, payload_budget::PayloadBudgetExceeded};
use serde_json::value::RawValue;
use std::{
    fmt::{Error as FormatError, Write},
    num::NonZeroUsize,
};

/// The default entity value limit.
const ENTITY_VALUES: usize = 1_000_000;

/// Decoding a body past the entity value limit may peak this far.
const DECODE_PEAK_BUDGET: u64 = 400 * 1024 * 1024;

/// Refusing the entity decoded from the body over the limit may peak this far
/// above it.
const REFUSAL_PEAK_BUDGET: u64 = 512 * 1024 * 1024;

/// A create body: `head`, `count` entries each written by `entry`, and `tail`.
fn body(head: &str, count: usize, tail: &str, entry: impl Fn(&mut String, usize) -> Result<(), FormatError>) -> Result<Box<RawValue>, FormatError> {
    let mut body = String::with_capacity(count * 24 + head.len() + tail.len());
    body.push_str(head);
    for number in 0..count {
        if number > 0 {
            body.push(',');
        }
        entry(&mut body, number)?;
    }
    body.push_str(tail);
    Ok(RawValue::from_string(body).expect("the body is JSON"))
}

/// The default entity value limit.
const fn entity_limit() -> JsonValueLimit {
    JsonValueLimit::new(NonZeroUsize::new(ENTITY_VALUES).expect("a positive limit"))
}

/// Assert that decoding `body` and walking it bounded by the entity limit is
/// refused with the entity value limit, the refusal holding little beyond the
/// entity decoded.
async fn assert_refused_within_budget(body: &RawValue) {
    let engine = JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds");
    let context = engine.expansion_context(&ContextSpec::none()).await.expect("the core context processes");
    let expander = TypedExpander::new(&context).bounded_by(entity_limit());
    let mut decoded = None;
    let mut walked = None;

    let decoding = allocation_counter::measure(|| {
        decoded = Some(
            Entity::<AttributeValue>::from_bounded_request_text(
                body.get().as_bytes(),
                ContextReading::keyword_spellings(),
                DecodeBounds::new(GeometryPositionLimit::new(NonZeroUsize::MIN.saturating_add(999_999)), entity_limit()),
            )
            .expect("the body is an Entity"),
        );
    });
    let entity = decoded.expect("the body decoded");
    let refusal = allocation_counter::measure(|| {
        walked = Some(expander.entity(&entity, Normalization::Entity));
    });

    println!(
        "a body of {} bytes: decoding peaked at {} bytes, refusing at {} bytes above the entity",
        body.get().len(),
        decoding.bytes_max,
        refusal.bytes_max
    );
    match walked.expect("the walk ran") {
        Err(ExpansionError::PayloadBudget(PayloadBudgetExceeded::EntityJsonValues { limit })) => assert_eq!(limit, entity_limit()),
        other => panic!("the entity limit refuses the body, got {:?}", other.map(|_| ())),
    }
    assert!(
        decoding.bytes_max < DECODE_PEAK_BUDGET,
        "decoding the body peaked at {} bytes, over the {DECODE_PEAK_BUDGET} byte budget",
        decoding.bytes_max
    );
    assert!(
        refusal.bytes_max < REFUSAL_PEAK_BUDGET,
        "refusing the body peaked at {} bytes above the entity, over the {REFUSAL_PEAK_BUDGET} byte budget",
        refusal.bytes_max
    );
}

#[tokio::test]
async fn a_relationship_of_millions_of_targets_is_refused_before_it_is_expanded() {
    let body = body(
        r#"{"id":"urn:ngsi-ld:Big:1","type":"Big","r":{"type":"Relationship","object":["#,
        2_000_000,
        "]}}",
        |text, number| write!(text, r#""urn:ngsi-ld:Target:{number}""#),
    )
    .expect("the body is written");
    assert_refused_within_budget(&body).await;
}

#[tokio::test]
async fn a_list_relationship_of_millions_of_objects_is_refused_before_it_is_expanded() {
    let body = body(
        r#"{"id":"urn:ngsi-ld:Big:1","type":"Big","r":{"type":"ListRelationship","objectList":["#,
        1_500_000,
        "]}}",
        |text, number| write!(text, r#""urn:ngsi-ld:Target:{number}""#),
    )
    .expect("the body is written");
    assert_refused_within_budget(&body).await;
}

#[tokio::test]
async fn a_vocab_property_of_millions_of_terms_is_refused_before_it_is_expanded() {
    let body = body(
        r#"{"id":"urn:ngsi-ld:Big:1","type":"Big","v":{"type":"VocabProperty","vocab":["#,
        2_000_000,
        "]}}",
        |text, number| write!(text, r#""term{number}""#),
    )
    .expect("the body is written");
    assert_refused_within_budget(&body).await;
}

#[tokio::test]
async fn a_language_map_of_millions_of_strings_under_one_tag_is_refused_before_it_is_expanded() {
    let body = body(
        r#"{"id":"urn:ngsi-ld:Big:1","type":"Big","l":{"type":"LanguageProperty","languageMap":{"en":["#,
        3_900_000,
        "]}}}",
        |text, _number| write!(text, r#""s""#),
    )
    .expect("the body is written");
    assert_refused_within_budget(&body).await;
}

#[tokio::test]
async fn a_language_map_of_millions_of_tags_is_refused_before_it_is_expanded() {
    let body = body(
        r#"{"id":"urn:ngsi-ld:Big:1","type":"Big","l":{"type":"LanguageProperty","languageMap":{"#,
        3_900_000,
        "}}}",
        |text, number| write!(text, r#""x-{number}":"s""#),
    )
    .expect("the body is written");
    assert_refused_within_budget(&body).await;
}

#[tokio::test]
async fn an_entity_scope_of_millions_of_scopes_is_refused_before_it_is_expanded() {
    let body = body(r#"{"id":"urn:ngsi-ld:Big:1","type":"Big","scope":["#, 3_900_000, "]}", |text, number| {
        write!(text, r#""/s{number}""#)
    })
    .expect("the body is written");
    assert_refused_within_budget(&body).await;
}
