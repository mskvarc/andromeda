//! A list relationship reformatted for 1 0 keeps every object.

use andromeda_test_suite::{
    body_json::{json_absent, json_equals},
    broker,
    namespace::Namespace,
    version_vehicle::{LabelInstances, create},
    versioned_answer::send_versioned,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{RepresentationSpelling, RetrieveEntity},
    },
    model::{Lossless, NgsiLdMediaType},
};

/// Pins: Retrieve Entity with Prefer ngsi-ld=1.0 answers 203 with a `ListRelationship` reformatted as a
/// Relationship whose object is every URI of its `objectList` in list order, in the normalized and the concise
/// representation
/// Clause: 4.3.6.8, 4.5.1, 4.5.3.3
#[gtest]
#[tokio::test]
async fn o_cmn_prf_9007_a_list_relationship_reformatted_for_1_0_keeps_every_object() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = create(&broker, &namespace, "list-relationship", LabelInstances::One).await?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    // Table 4.3.6.8-3 types the `object` of a Relationship as String or String[], so every target is kept in list order.
    let rooms = vec![format!("urn:ngsi-ld:Room:{namespace}-first"), format!("urn:ngsi-ld:Room:{namespace}-second")];
    let normalized = RetrieveEntity::builder()
        .id(&vehicle_id)
        .format(Lossless::Normalized)
        .spelling(RepresentationSpelling::default().in_options().explicit_normalized())
        .build();
    // The answers are adapted to an older version, so their wire form is held as received.
    let answer = send_versioned(&json, &normalized, Some("1.0")).await?;
    expect_that!(answer.status, eq(StatusCode::NON_AUTHORITATIVE_INFORMATION));
    expect_that!(answer.preference_applied.as_deref(), some(eq("ngsi-ld=1.0")));
    expect_that!(&answer.body, json_equals("$.refRooms.type", "Relationship"));
    expect_that!(&answer.body, json_equals("$.refRooms.object", rooms.clone()));
    expect_that!(&answer.body, json_absent("$.refRooms.objectList"));
    let concise = RetrieveEntity::builder()
        .id(&vehicle_id)
        .format(Lossless::Concise)
        .spelling(RepresentationSpelling::default().in_options())
        .build();
    let answer_2 = send_versioned(&json, &concise, Some("1.0")).await?;
    expect_that!(answer_2.status, eq(StatusCode::NON_AUTHORITATIVE_INFORMATION));
    expect_that!(answer_2.preference_applied.as_deref(), some(eq("ngsi-ld=1.0")));
    expect_that!(&answer_2.body, json_equals("$.refRooms.object", rooms));
    expect_that!(&answer_2.body, json_absent("$.refRooms.objectList"));
    Ok(())
}
