//! A Property value of millions of numbers costs the broker a bounded amount
//! of memory (clause 6.3.2).
//!
//! A body of 3,900,000 numbers in one Property stays under the default
//! request value limit of 4,000,000 and is far over the entity value limit of
//! 1,000,000. Decoding it into the typed Entity and the walk bounded by the
//! entity limit together peak below 512 MiB above the body: the walk refuses
//! the entity once its count passes the limit, before it expands the rest. An
//! entity of 1,000,000 numbers, within the limit, expands into a document of
//! at most 128 bytes per value, and the expansion peaks below 256 MiB.
//!
//! The measurement counts the allocations of the calling thread alone, so the
//! other tests of the binary do not disturb it.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
use andromeda_json_literal::piece::JsonPiece;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_expansion::{error::ExpansionError, normalization::Normalization, typed_expander::TypedExpander, typed_expansion::TypedExpansion};
use andromeda_ngsi_ld_model::{attribute::instance::AttributeValue, entity::Entity};
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use andromeda_ngsi_ld_representation::{compact_node::produced_node::ProducedNode, payload_budget::PayloadBudgetExceeded};
use serde_json::value::RawValue;
use std::{
    fmt::{Error as FormatError, Write},
    num::NonZeroUsize,
};

/// The default entity value limit.
const ENTITY_VALUES: usize = 1_000_000;

/// The numbers of the body over the entity limit, under the request limit.
const OVER_THE_LIMIT: usize = 3_900_000;

/// Decoding and expanding the body over the limit may peak this far above it.
const REFUSAL_PEAK_BUDGET: u64 = 512 * 1024 * 1024;

/// Expanding an entity of [`ENTITY_VALUES`] numbers may peak this high.
const EXPANSION_PEAK_BUDGET: u64 = 256 * 1024 * 1024;

/// The document an entity of [`ENTITY_VALUES`] numbers expands to may hold
/// this many bytes per number.
const DOCUMENT_BYTES_PER_VALUE: i64 = 128;

/// A create body whose one Property holds the numbers `0..numbers`.
fn body(numbers: usize) -> Result<Box<RawValue>, FormatError> {
    let mut body = String::with_capacity(numbers * 8 + 128);
    body.push_str(r#"{"id":"urn:ngsi-ld:Big:1","type":"Big","v":{"type":"Property","value":["#);
    for number in 0..numbers {
        if number > 0 {
            body.push(',');
        }
        write!(body, "{number}")?;
    }
    body.push_str("]}}");
    Ok(RawValue::from_string(body).expect("the body is JSON"))
}

/// The body read into the typed Entity, as the transport reads it.
fn decoded(body: &RawValue) -> Entity<AttributeValue> {
    Entity::from_node(ProducedNode::new(JsonPiece::new(body))).expect("the body is an Entity")
}

/// The default entity value limit.
const fn entity_limit() -> JsonValueLimit {
    JsonValueLimit::new(NonZeroUsize::new(ENTITY_VALUES).expect("a positive limit"))
}

#[tokio::test]
async fn a_property_of_millions_of_numbers_is_refused_before_it_is_expanded() {
    let engine = JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds");
    let context = engine.expansion_context(&ContextSpec::none()).await.expect("the core context processes");
    let body = body(OVER_THE_LIMIT).expect("the body is written");
    let expander = TypedExpander::new(&context).bounded_by(entity_limit());
    let mut walked = None;

    let measured = allocation_counter::measure(|| {
        walked = Some(expander.entity(&decoded(&body), Normalization::Entity));
    });

    let peak_above_input = measured.bytes_max;
    println!("refusing {OVER_THE_LIMIT} numbers peaked at {peak_above_input} bytes above the body");
    match walked.expect("the walk ran") {
        Err(ExpansionError::PayloadBudget(PayloadBudgetExceeded::EntityJsonValues { limit })) => assert_eq!(limit, entity_limit()),
        other => panic!("the entity limit refuses the body, got {:?}", other.map(|_| ())),
    }
    assert!(
        peak_above_input < REFUSAL_PEAK_BUDGET,
        "refusing {OVER_THE_LIMIT} numbers peaked at {peak_above_input} bytes above the body, over the {REFUSAL_PEAK_BUDGET} byte budget"
    );
}

#[tokio::test]
async fn a_property_of_a_million_numbers_expands_into_a_compact_document() {
    let engine = JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds");
    let context = engine.expansion_context(&ContextSpec::none()).await.expect("the core context processes");
    let entity = decoded(&body(ENTITY_VALUES).expect("the body is written"));
    let expander = TypedExpander::new(&context).bounded_by(entity_limit());
    let mut walked = None;

    let measured = allocation_counter::measure(|| {
        walked = Some(expander.entity(&entity, Normalization::Entity).expect("a million numbers are within the limit"));
    });

    let peak = measured.bytes_max;
    let held = measured.bytes_current;
    println!("expanding {ENTITY_VALUES} numbers peaked at {peak} bytes and holds {held} bytes");
    let Some(TypedExpansion::Expanded(_document)) = walked else {
        panic!("the walk expands the entity itself");
    };
    assert!(
        peak < EXPANSION_PEAK_BUDGET,
        "expanding {ENTITY_VALUES} numbers peaked at {peak} bytes, over {EXPANSION_PEAK_BUDGET}"
    );
    let budget = DOCUMENT_BYTES_PER_VALUE * i64::try_from(ENTITY_VALUES).expect("a small count");
    assert!(held < budget, "the document of {ENTITY_VALUES} numbers holds {held} bytes, over {budget}");
}
