//! A Property of millions of numbers under a context the typed walk declines
//! costs the broker a bounded amount of memory (clause 6.3.2).
//!
//! The body stays under the default request value limit of 4,000,000 and is far
//! over the entity value limit of 1,000,000. The walk declines the attribute
//! before it counts a value, so the JSON-LD processor, which counts nothing,
//! would expand the whole array; the count taken from the typed model refuses
//! the write first, and the refusal peaks far below 512 MiB above the entity it
//! reads.
//!
//! The measurement counts the allocations of the calling thread alone, so the
//! other tests of the binary do not disturb it.

#![expect(
    clippy::expect_used,
    reason = "clippy reaches only `#[test]` functions in a member crate's integration test, and these helpers fail the test by panicking"
)]
use andromeda_json_literal::{JsonLiteral, piece::JsonPiece};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_expansion::{
    decline::Decline,
    entity_expansion::EntityExpansion,
    error::ExpansionError,
    normalization::Normalization,
    typed_expander::TypedExpander,
    typed_expansion::TypedExpansion,
};
use andromeda_ngsi_ld_model::{attribute::instance::AttributeValue, entity::Entity};
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use andromeda_ngsi_ld_representation::{compact_node::produced_node::ProducedNode, payload_budget::PayloadBudgetExceeded};
use serde_json::value::RawValue;
use std::{fmt::Write, num::NonZeroUsize};
use tokio::runtime::Builder;

/// The default entity value limit.
const ENTITY_VALUES: usize = 1_000_000;

/// Refusing the entity decoded from the body over the limit may peak this far
/// above it.
const REFUSAL_PEAK_BUDGET: u64 = 64 * 1024 * 1024;

/// A body context giving the attribute `v` a scoped context that does not
/// propagate, which the walk declines.
const DECLINING_CONTEXT: &str = r#"[{"v":{"@id":"https://example.org/declined/v","@context":{"@propagate":false}}}]"#;

/// The default entity value limit.
const fn entity_limit() -> JsonValueLimit {
    JsonValueLimit::new(NonZeroUsize::new(ENTITY_VALUES).expect("a positive limit"))
}

/// A create body holding one Property of `count` numbers.
fn body(count: usize) -> Box<RawValue> {
    let mut body = String::with_capacity(count * 8 + 96);
    body.push_str(r#"{"id":"urn:ngsi-ld:Big:1","type":"Big","v":{"type":"Property","value":["#);
    for number in 0..count {
        if number > 0 {
            body.push(',');
        }
        write!(body, "{number}").expect("a number is written");
    }
    body.push_str("]}}");
    RawValue::from_string(body).expect("the body is JSON")
}

#[test]
fn a_property_of_millions_of_numbers_under_a_declined_context_is_refused_before_the_processor_expands_it() {
    let runtime = Builder::new_current_thread().build().expect("a runtime");
    let engine = JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds");
    let spec = ContextSpec::from_body(JsonLiteral::admit(DECLINING_CONTEXT).expect("an admissible context"));
    let context = runtime.block_on(engine.expansion_context(&spec)).expect("the declining context processes");
    let body = body(3_900_000);
    let entity = Entity::<AttributeValue>::from_node(ProducedNode::new(JsonPiece::new(&body))).expect("the body is an Entity");

    let walked = TypedExpander::new(&context)
        .bounded_by(entity_limit())
        .entity(&entity, Normalization::Entity)
        .expect("the walk declines");
    assert_eq!(walked, TypedExpansion::Declined(Decline::NonPropagatedContext));

    let expansion = EntityExpansion::from_context(&engine, context).bounded_by(entity_limit());
    let mut expanded = None;
    let refusal = allocation_counter::measure(|| {
        expanded = Some(runtime.block_on(expansion.entity(&entity, Normalization::Entity)));
    });

    println!("a body of {} bytes: refusing at {} bytes above the entity", body.get().len(), refusal.bytes_max);
    match expanded.expect("the expansion ran") {
        Err(ExpansionError::PayloadBudget(PayloadBudgetExceeded::EntityJsonValues { limit })) => assert_eq!(limit, entity_limit()),
        other => panic!("the entity limit refuses the body, got {:?}", other.map(|_| ())),
    }
    assert!(
        refusal.bytes_max < REFUSAL_PEAK_BUDGET,
        "refusing the body peaked at {} bytes above the entity, over the {REFUSAL_PEAK_BUDGET} byte budget",
        refusal.bytes_max
    );
}
