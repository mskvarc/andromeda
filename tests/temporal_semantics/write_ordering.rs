//! Operational ordering and same-batch ordering.
//!
//! Current state follows the order writes committed in, while a temporal query
//! follows the semantic axis it names. The operational clock that decides the
//! first is never the axis that decides the second, and inside one batch the
//! deterministic ordinal — not row order — is what makes a reconstruction
//! repeatable.

use crate::{
    attribute_iri,
    clean_database,
    client,
    create_entity,
    create_entity_batch,
    database,
    get_entity,
    get_temporal_entity,
    rendered_instances,
    stored_history::{stored_attribute_history, stored_history},
    unique_id,
    update_entity_attributes,
    upsert_entity_batch,
};
use reqwest::StatusCode;
use serde_json::json;
use serial_test::serial;
use std::collections::BTreeSet;

/// The instant the first write declares as its observation.
const LATER_OBSERVATION: &str = "2021-03-01T00:00:00Z";

/// The backdated instant the second write declares.
const EARLIER_OBSERVATION: &str = "2021-01-01T00:00:00Z";

/// A backdated update still wins current state, because current state follows
/// commit order while the observed-time axis follows what was declared.
#[tokio::test]
#[serial]
async fn temporal_semantics_operational_ordering_separates_commit_order_from_observed_order() {
    let _serialized_environment = clean_database().await;
    let http = client();
    let pool = database().await;
    let entity_id = unique_id("urn:ngsi-ld:Vehicle:operational_ordering");

    let created = create_entity(
        &http,
        &json!({
            "id": entity_id,
            "type": "Vehicle",
            "speed": { "type": "Property", "value": 10, "observedAt": LATER_OBSERVATION }
        }),
    )
    .await;
    assert_eq!(created, StatusCode::CREATED);

    let updated = update_entity_attributes(
        &http,
        &entity_id,
        &json!({ "speed": { "type": "Property", "value": 20, "observedAt": EARLIER_OBSERVATION } }),
    )
    .await;
    assert_eq!(updated, StatusCode::NO_CONTENT);

    // Current state is the last committed value, not the latest observation.
    let (status, entity) = get_entity(&http, &entity_id).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(entity["speed"]["value"], json!(20), "current state follows committed write order");

    let rows = stored_attribute_history(&pool, &entity_id, &attribute_iri("speed")).await;
    assert_eq!(rows.len(), 2, "both writes recorded an instance");
    assert_ne!(rows[0].write_id, rows[1].write_id, "two separate writes carry two identities");
    assert!(
        rows[0].recorded_at < rows[1].recorded_at,
        "the write clock orders the rows by the order they committed in"
    );
    assert_eq!(rows[0].value_number.as_deref(), Some("10"));
    assert_eq!(rows[1].value_number.as_deref(), Some("20"));
    assert!(
        rows[1].observed_at < rows[0].observed_at,
        "the second write declares the earlier observation, so the two axes disagree"
    );

    // The observed-time axis answers by what was declared, in both directions.
    for (start, end, expected) in [
        ("2020-12-31T00:00:00Z", "2021-01-02T00:00:00Z", 20),
        ("2021-02-28T00:00:00Z", "2021-03-02T00:00:00Z", 10),
    ] {
        let (status, body) = get_temporal_entity(&http, &entity_id, &format!("timerel=between&timeAt={start}&endTimeAt={end}")).await;
        assert_eq!(status, StatusCode::OK, "the {start} window is a valid query");
        let matched = rendered_instances(&body, "speed");
        assert_eq!(matched.len(), 1, "the {start} window holds exactly one declared observation");
        assert_eq!(
            matched[0]["value"],
            json!(expected),
            "the {start} window returns the value declared at that instant"
        );
    }
}

/// Every consequence of one batch shares its ingestion instant, and each write
/// inside it numbers its consequences uniquely and densely from zero.
#[tokio::test]
#[serial]
async fn temporal_semantics_same_batch_ordering_is_unique_and_deterministic() {
    let _serialized_environment = clean_database().await;
    let http = client();
    let pool = database().await;
    let first_id = unique_id("urn:ngsi-ld:Vehicle:batch_ordering_first");
    let second_id = unique_id("urn:ngsi-ld:Vehicle:batch_ordering_second");

    let created = create_entity_batch(
        &http,
        &json!([
            { "id": first_id, "type": "Vehicle", "speed": { "type": "Property", "value": 1 }, "fuel": { "type": "Property", "value": 2 } },
            { "id": second_id, "type": "Vehicle", "speed": { "type": "Property", "value": 3 }, "fuel": { "type": "Property", "value": 4 } }
        ]),
    )
    .await;
    assert_eq!(created, StatusCode::CREATED);

    let first_rows = stored_history(&pool, &first_id).await;
    let second_rows = stored_history(&pool, &second_id).await;
    assert_eq!(first_rows.len(), 2);
    assert_eq!(second_rows.len(), 2);

    // One batch is one ingestion instant, whatever it decomposes into.
    let instants = first_rows.iter().chain(second_rows.iter()).map(|row| row.recorded_at).collect::<BTreeSet<_>>();
    assert_eq!(instants.len(), 1, "every consequence of one batch shares one recorded_at");

    // Inside one write the ordinals are unique and dense, so a replay of the
    // write reproduces exactly the same key for exactly the same consequence.
    for rows in [&first_rows, &second_rows] {
        let identities = rows.iter().map(|row| row.write_id).collect::<BTreeSet<_>>();
        assert_eq!(identities.len(), 1, "one Entity's consequences share one write identity");
        let ordinals = rows.iter().map(|row| row.write_ordinal).collect::<BTreeSet<_>>();
        assert_eq!(ordinals.len(), rows.len(), "ordinals inside one write are unique");
        assert_eq!(
            ordinals.iter().copied().collect::<Vec<i32>>(),
            (0..i32::try_from(rows.len()).expect("a two-row write is representable")).collect::<Vec<i32>>(),
            "ordinals start at zero and leave no gap"
        );
    }

    // The two writes are independent identities, so nothing about the batch
    // makes one Entity's ordinals collide with another's.
    assert_ne!(first_rows[0].write_id, second_rows[0].write_id);

    // Reconstruction picks the values the batch specified, per Entity.
    for (entity_id, speed, fuel) in [(&first_id, 1, 2), (&second_id, 3, 4)] {
        let (status, entity) = get_entity(&http, entity_id).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(entity["speed"]["value"], json!(speed));
        assert_eq!(entity["fuel"]["value"], json!(fuel));
    }
}

/// A later change inside the same batch is the one that survives, and the
/// choice does not depend on the order rows happen to be stored in.
#[tokio::test]
#[serial]
async fn temporal_semantics_same_batch_reconstruction_takes_the_specified_final_change() {
    let _serialized_environment = clean_database().await;
    let http = client();
    let pool = database().await;
    let entity_id = unique_id("urn:ngsi-ld:Vehicle:batch_final_change");

    // The same Entity appears twice in one batch; the later member is the
    // change the batch specifies, and the choice must not depend on the order
    // the rows happen to come back in.
    let upserted = upsert_entity_batch(
        &http,
        &json!([
            { "id": entity_id, "type": "Vehicle", "speed": { "type": "Property", "value": 2 } },
            { "id": entity_id, "type": "Vehicle", "speed": { "type": "Property", "value": 3 } }
        ]),
    )
    .await;
    assert_eq!(upserted, StatusCode::CREATED);

    let (status, entity) = get_entity(&http, &entity_id).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(entity["speed"]["value"], json!(3), "the final specified change is the projected one");

    let rows = stored_attribute_history(&pool, &entity_id, &attribute_iri("speed")).await;
    assert_eq!(rows.len(), 1, "the batch resolves to the one change it specifies");
    assert_eq!(
        rows[0].value_number.as_deref(),
        Some("3"),
        "history records the same change the projection holds"
    );
    assert_eq!(rows[0].write_ordinal, 0, "the surviving consequence keeps a deterministic ordinal");
}
