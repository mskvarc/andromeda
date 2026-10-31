//! Version 1 0 reformats every normalized family.

use andromeda_test_suite::{
    body_json::{json_absent, json_count, json_equals},
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

/// Pins: Retrieve Entity with options=normalized and Prefer ngsi-ld=1.0 answers 203 with each later family
/// reformatted as a Property or a Relationship holding its value, the 1.3 members removed and the default instance
/// alone
/// Clause: 4.3.6.8, 4.5.1
#[gtest]
#[tokio::test]
async fn n_cmn_prf_9006_version_1_0_reformats_every_normalized_family() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = create(&broker, &namespace, "normalized", LabelInstances::One).await?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let normalized = RetrieveEntity::builder()
        .id(&vehicle_id)
        .format(Lossless::Normalized)
        .spelling(RepresentationSpelling::default().in_options().explicit_normalized())
        .build();
    // The answer is adapted to an older version, so its wire form is held as received.
    let answer = send_versioned(&json, &normalized, Some("1.0")).await?;
    expect_that!(answer.status, eq(StatusCode::NON_AUTHORITATIVE_INFORMATION));
    expect_that!(answer.preference_applied.as_deref(), some(eq("ngsi-ld=1.0")));
    expect_that!(&answer.body, json_equals("$.speed.type", "Property"));
    expect_that!(&answer.body, json_equals("$.speed.value", 21));
    expect_that!(&answer.body, json_absent("$.speed.observedAt"));
    expect_that!(&answer.body, json_absent("$.speed.unitCode"));
    expect_that!(&answer.body, json_absent("$.speed.valueType"));
    expect_that!(&answer.body, json_equals("$.label.type", "Property"));
    expect_that!(&answer.body, json_equals("$.label.value.en", "Store"));
    expect_that!(&answer.body, json_equals("$.label.value.de", "Laden"));
    expect_that!(&answer.body, json_absent("$.label.languageMap"));
    expect_that!(&answer.body, json_equals("$.category.type", "Property"));
    expect_that!(&answer.body, json_equals("$.category.value", "Building"));
    expect_that!(&answer.body, json_equals("$.config.type", "Property"));
    expect_that!(&answer.body, json_equals("$.config.value.a", 1));
    expect_that!(&answer.body, json_equals("$.tags.type", "Property"));
    expect_that!(&answer.body, json_count("$.tags.value", 2));
    expect_that!(&answer.body, json_equals("$.tags.value[1]", "b"));
    expect_that!(&answer.body, json_equals("$.refBuilding.type", "Relationship"));
    expect_that!(
        &answer.body,
        json_equals("$.refBuilding.object", format!("urn:ngsi-ld:Building:{namespace}-target"))
    );
    expect_that!(&answer.body, json_absent("$.refBuilding.objectType"));
    expect_that!(&answer.body, json_absent("$.refBuilding.observedAt"));
    expect_that!(&answer.body, json_equals("$.refRooms.type", "Relationship"));
    // Table 4.3.6.8-3 types the `object` of a Relationship as String or String[], so every target is kept in list order.
    expect_that!(
        &answer.body,
        json_equals(
            "$.refRooms.object",
            vec![format!("urn:ngsi-ld:Room:{namespace}-first"), format!("urn:ngsi-ld:Room:{namespace}-second")]
        )
    );
    expect_that!(&answer.body, json_equals("$.pressure.type", "Property"));
    expect_that!(&answer.body, json_equals("$.pressure.value", 30));
    expect_that!(&answer.body, json_absent("$.pressure.datasetId"));
    Ok(())
}
