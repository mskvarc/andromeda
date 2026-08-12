//! Verification matrix row "Timestamp precision policy".
//!
//! `docs/timestamp-precision.md` records the policy: every NGSI-LD instant is
//! truncated to microseconds in Rust, before persistence, so the value the
//! database stores is the value the broker decided on rather than one the
//! column rounded on its own; and an instant outside the range the broker can
//! represent is refused with a precise error rather than stored approximately.
//!
//! The unit tests cover the truncation function. What this file adds is the
//! end-to-end half the matrix row states: what a client sends decides what
//! storage holds, nothing between them rounds, and an out-of-range date never
//! reaches storage at all.
//!
//! Rendering is the one place the two resolutions part company, and
//! deliberately: `andromeda_ngsi_ld_lexicon::rfc3339` renders every timestamp
//! the broker emits at whole-second precision, which is a stable outbound
//! format rather than a claim about stored precision. The assertions below
//! therefore compare the rendering against the whole-second form of the stored
//! instant — a rendering that disagreed with that would mean a value had been
//! rounded somewhere, which is exactly what the policy exists to prevent.

use crate::{
    attribute_iri,
    clean_database,
    client,
    create_entity,
    database,
    get_temporal_entity,
    rendered_instances,
    stored_history::stored_attribute_history,
    unique_id,
};
use andromeda_ngsi_ld_lexicon::rfc3339::whole_seconds;
use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use serde_json::{Value, json};
use serial_test::serial;

/// Fractional digits below the microsecond the broker resolves to.
///
/// The trailing `789` is what a naive `TIMESTAMPTZ` cast would round rather
/// than drop, which is the difference the policy exists to remove.
const SUB_MICROSECOND: &str = "2026-03-04T05:06:07.123456789Z";

/// The instant `SUB_MICROSECOND` truncates to.
const TRUNCATED: &str = "2026-03-04T05:06:07.123456Z";

/// An instant a microsecond-resolution timestamp represents exactly.
const EXACT_MICROSECOND: &str = "2026-03-04T05:06:07.123456Z";

/// Instants outside the range `docs/timestamp-precision.md` declares storable:
/// below Julian day zero, and above chrono's year ceiling of 262142.
const OUT_OF_RANGE: &[&str] = &["-4714-01-01T00:00:00Z", "300000-01-01T00:00:00Z"];

/// One entity whose Property declares `observedAt` and carries a DateTime value.
fn entity_with_instants(entity_id: &str, observed_at: &str, value: &str) -> Value {
    json!({
        "id": entity_id,
        "type": "Vehicle",
        "inspected": {
            "type": "Property",
            "value": {"@type": "DateTime", "@value": value},
            "observedAt": observed_at
        }
    })
}

/// The instant a stored timestamp holds, as an RFC 3339 string.
fn stored_instant(instant: Option<DateTime<Utc>>) -> String {
    instant
        .unwrap_or_else(|| unreachable!("the axis the fixture declared must be stored"))
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

/// Clause 4.6.3 types an NGSI-LD DateTime as an RFC 3339 instant and puts no
/// resolution on it; the broker resolves to microseconds and says so. A
/// sub-microsecond value is therefore truncated once, in Rust, and both storage
/// and rendering agree on the truncated instant.
#[tokio::test]
#[serial]
async fn temporal_semantics_sub_microsecond_instants_are_truncated_once_before_persistence() {
    let _serialized_environment = clean_database().await;
    let http = client();
    let pool = database().await;
    let entity_id = unique_id("urn:ngsi-ld:Vehicle:timestamp_precision");

    let created = create_entity(&http, &entity_with_instants(&entity_id, SUB_MICROSECOND, SUB_MICROSECOND)).await;
    assert_eq!(created, StatusCode::CREATED, "the sub-microsecond fixture must be accepted");

    let rows = stored_attribute_history(&pool, &entity_id, &attribute_iri("inspected")).await;
    assert_eq!(rows.len(), 1, "the fixture records exactly one observation");
    let expected = TRUNCATED.parse::<DateTime<Utc>>().expect("a valid expected instant");
    assert_eq!(
        stored_instant(rows[0].observed_at),
        expected.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
        "the stored observedAt is the truncated instant, not a rounded one"
    );

    let (status, body) = get_temporal_entity(&http, &entity_id, "timerel=after&timeAt=2000-01-01T00:00:00Z").await;
    assert_eq!(status, StatusCode::OK, "the temporal representation must answer: {body}");
    let instances = rendered_instances(&body, "inspected");
    assert_eq!(instances.len(), 1, "one observation renders: {body}");
    assert_eq!(
        instances[0]["observedAt"].as_str().unwrap_or_default(),
        whole_seconds(expected),
        "the rendering is the broker's whole-second form of the instant storage holds, not a rounded value: {body}"
    );
}

/// A value already at the broker's resolution is stored and rendered unchanged,
/// which is what makes the truncation above a decision rather than a loss of
/// every fractional digit.
#[tokio::test]
#[serial]
async fn temporal_semantics_a_microsecond_instant_round_trips_exactly() {
    let _serialized_environment = clean_database().await;
    let http = client();
    let pool = database().await;
    let entity_id = unique_id("urn:ngsi-ld:Vehicle:timestamp_exact");

    let created = create_entity(&http, &entity_with_instants(&entity_id, EXACT_MICROSECOND, EXACT_MICROSECOND)).await;
    assert_eq!(created, StatusCode::CREATED, "the microsecond fixture must be accepted");

    let expected = EXACT_MICROSECOND.parse::<DateTime<Utc>>().expect("a valid expected instant");
    let rows = stored_attribute_history(&pool, &entity_id, &attribute_iri("inspected")).await;
    assert_eq!(rows.len(), 1, "the fixture records exactly one observation");
    assert_eq!(
        stored_instant(rows[0].observed_at),
        expected.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
        "a microsecond instant survives storage unchanged"
    );

    let (status, body) = get_temporal_entity(&http, &entity_id, "timerel=after&timeAt=2000-01-01T00:00:00Z").await;
    assert_eq!(status, StatusCode::OK, "the temporal representation must answer: {body}");
    let instances = rendered_instances(&body, "inspected");
    assert_eq!(instances.len(), 1, "one observation renders: {body}");
    assert_eq!(
        instances[0]["observedAt"].as_str().unwrap_or_default(),
        whole_seconds(expected),
        "a microsecond instant renders through the whole-second outbound form: {body}"
    );
}

/// An instant outside the representable range is invalid input (clause 5.5.9),
/// so the write is refused and nothing about it reaches storage.
#[tokio::test]
#[serial]
async fn temporal_semantics_an_out_of_range_instant_is_refused_rather_than_stored() {
    let _serialized_environment = clean_database().await;
    let http = client();
    let pool = database().await;

    for out_of_range in OUT_OF_RANGE {
        let entity_id = unique_id("urn:ngsi-ld:Vehicle:timestamp_range");
        let created = create_entity(&http, &entity_with_instants(&entity_id, out_of_range, EXACT_MICROSECOND)).await;
        assert_ne!(created, StatusCode::CREATED, "{out_of_range} is not a representable instant");
        assert!(
            stored_attribute_history(&pool, &entity_id, &attribute_iri("inspected")).await.is_empty(),
            "a refused {out_of_range} write must leave no history row behind"
        );
    }
}
