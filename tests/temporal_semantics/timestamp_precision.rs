//! Verification matrix row "Timestamp precision policy".
//!
//! `docs/timestamp-precision.md` records the policy: a client-supplied NGSI-LD
//! instant is accepted exactly as clause 4.6.3 defines it — the literal `Z`
//! designator, at most six fractional-second digits — and anything else is
//! refused with a precise error rather than normalised, truncated or stored
//! approximately. An instant outside the range the broker can represent is
//! refused for the same reason.
//!
//! The unit tests cover the parser. What this file adds is the end-to-end half
//! the matrix row states: what a client sends decides what storage holds,
//! nothing between them rounds, and a value the policy refuses never reaches
//! storage at all.
//!
//! Rendering the temporal representation carries the stored instant back in the
//! form `andromeda_ngsi_ld_lexicon::rfc3339::sub_second` produces — exactly the
//! sub-second precision the instant carries, up to the microsecond resolution
//! the `TIMESTAMPTZ` column holds and clause 4.6.3 caps a `DateTime` at. The
//! assertions below compare the rendering against that form of the stored
//! instant — a rendering that disagreed with it would mean a value had been
//! rounded or truncated somewhere, which is exactly what the policy exists to
//! prevent.

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
use andromeda_ngsi_ld_lexicon::rfc3339::sub_second;
use chrono::{DateTime, Utc};
use pretty_assertions::{assert_eq, assert_ne};
use reqwest::StatusCode;
use serde_json::{Value, json};
use serial_test::serial;

/// Fractional digits below the microsecond the broker resolves to.
///
/// The trailing `789` is what a naive `TIMESTAMPTZ` cast would round rather
/// than drop, and what a truncating broker would silently discard. Clause 4.6.3
/// admits six digits, so this value is neither: it is refused.
const SUB_MICROSECOND: &str = "2026-03-04T05:06:07.123456789Z";

/// An instant a microsecond-resolution timestamp represents exactly.
const EXACT_MICROSECOND: &str = "2026-03-04T05:06:07.123456Z";

/// Instants outside the range `docs/timestamp-precision.md` declares storable:
/// below Julian day zero, and above chrono's year ceiling of 262142.
const OUT_OF_RANGE: &[&str] = &["-4714-01-01T00:00:00Z", "300000-01-01T00:00:00Z"];

/// One entity whose Property declares `observedAt` and carries a `DateTime` value.
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
        .expect("the axis the fixture declared must be stored")
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

/// Clause 4.6.3 caps an NGSI-LD `DateTime` at six fractional-second digits. A
/// finer value is refused rather than truncated: discarding digits the client
/// wrote is a policy of the broker's own invention, and the clause's explicit
/// ceiling exists precisely to remove that ambiguity. Nothing about the refused
/// write reaches storage, on either axis that carries a client instant.
#[tokio::test]
#[serial]
async fn temporal_semantics_sub_microsecond_instants_are_refused_rather_than_truncated() {
    let _serialized_environment = clean_database().await;
    let http = client();
    let pool = database().await;

    // Each axis is exercised on its own, so neither can be the one carrying the
    // refusal for both: the `observedAt` of clause 5.2.5 and a DateTime-typed
    // Property value are held to the same policy.
    for (observed_at, value) in [(SUB_MICROSECOND, EXACT_MICROSECOND), (EXACT_MICROSECOND, SUB_MICROSECOND)] {
        let entity_id = unique_id("urn:ngsi-ld:Vehicle:timestamp_precision");
        let created = create_entity(&http, &entity_with_instants(&entity_id, observed_at, value)).await;
        assert_eq!(
            created,
            StatusCode::BAD_REQUEST,
            "nine fractional digits exceed clause 4.6.3 and must be refused"
        );
        assert!(
            stored_attribute_history(&pool, &entity_id, &attribute_iri("inspected")).await.is_empty(),
            "a refused write must leave no history row behind"
        );
    }
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
        sub_second(expected),
        "a microsecond instant renders through the sub-second outbound form: {body}"
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
