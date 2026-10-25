//! A Temporal Evolution a client writes is decoded from its text into the
//! typed model without a JSON tree of it (clause 5.6.11): what the decode
//! allocates is bounded by the instances the body writes, a fixed number each,
//! and not by a tree whose size grows with every node of the text.
//!
//! The measurement counts the allocations of the calling thread alone, so the
//! other tests of the binary do not disturb it.

#![expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
use andromeda_ngsi_ld_model::{request_body::RequestBody, temporal::written_temporal_entity::WrittenTemporalEntity};
use andromeda_ngsi_ld_representation::core_term_aliases::CoreTermAliases;
use std::fmt::Write;

/// The attributes the created Temporal Evolution writes.
const ATTRIBUTES: usize = 3;

/// The instances each attribute holds.
const INSTANCES: usize = 1_000;

/// What one instance costs the decode at most: its own typed members, and no
/// allocation for any JSON node of the text it was read from.
const ALLOCATIONS_PER_INSTANCE: u64 = 24;

#[test]
fn a_temporal_body_is_decoded_without_a_tree() {
    // The text of a creation with `ATTRIBUTES` attributes of `instances`
    // instances each.
    let body = |instances: usize| {
        let mut text = String::from(r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle""#);
        for attribute in 0..ATTRIBUTES {
            write!(text, r#","attribute{attribute}":["#).expect("a string is written");
            for instance in 0..instances {
                if instance > 0 {
                    text.push(',');
                }
                write!(
                    text,
                    r#"{{"type":"Property","value":{instance},"observedAt":"2024-01-01T00:00:00Z","instanceId":"urn:ngsi-ld:instance:{attribute}:{instance}","unitCode":"CEL"}}"#
                )
                .expect("a string is written");
            }
            text.push(']');
        }
        text.push('}');
        text
    };
    // How many allocations decoding `text` makes, and how many instances it holds.
    let decode = |text: &str| {
        let mut decoded = None;
        let measured = allocation_counter::measure(|| {
            decoded = Some(WrittenTemporalEntity::from_request_text(text.as_bytes(), CoreTermAliases::keyword_spellings()).expect("the body decodes"));
        });
        let instances = decoded.expect("the decode ran").attrs.values().map(Vec::len).sum::<usize>();
        (measured.count_total, instances)
    };

    let (allocations, instances) = decode(&body(INSTANCES));
    assert_eq!(instances, ATTRIBUTES * INSTANCES);
    assert!(
        allocations <= ALLOCATIONS_PER_INSTANCE * u64::try_from(instances).expect("an instance count fits in u64"),
        "decoding {instances} instances made {allocations} allocations, over {ALLOCATIONS_PER_INSTANCE} per instance"
    );

    // Half the instances cost half the allocations: nothing grows with the
    // size of the whole text.
    let (half_allocations, half_instances) = decode(&body(INSTANCES / 2));
    assert_eq!(half_instances * 2, instances);
    assert!(
        allocations <= half_allocations * 2 + half_allocations / 20,
        "{half_instances} instances made {half_allocations} allocations and {instances} made {allocations}"
    );
}
