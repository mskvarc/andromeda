//! Version 1 0 collapses a reduced concise property to its bare value.

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

/// Pins: Retrieve Entity with options=concise answers the `VocabProperty` as a vocab object, and with Prefer
/// ngsi-ld=1.0 answers 203 with each Property reduced to its bare value, the reformatted families bare, the
/// Relationship as an object member alone and the default instance alone
/// Clause: 4.3.6.8, 4.5.2.3, 4.5.3.3, 4.5.5
#[gtest]
#[tokio::test]
async fn n_cmn_prf_9005_version_1_0_collapses_a_reduced_concise_property_to_its_bare_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = create(&broker, &namespace, "concise", LabelInstances::One).await?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let concise = RetrieveEntity::builder()
        .id(&vehicle_id)
        .format(Lossless::Concise)
        .spelling(RepresentationSpelling::default().in_options())
        .build();
    // The answers are adapted to an older version, so their wire form is held as received.
    let current = send_versioned(&json, &concise, None).await?;
    expect_that!(current.status, eq(StatusCode::OK));
    expect_that!(&current.body, json_equals("$.category.vocab", "Building"));
    let adapted = send_versioned(&json, &concise, Some("1.0")).await?;
    expect_that!(adapted.status, eq(StatusCode::NON_AUTHORITATIVE_INFORMATION));
    expect_that!(adapted.preference_applied.as_deref(), some(eq("ngsi-ld=1.0")));
    expect_that!(&adapted.body, json_equals("$.speed", 21));
    expect_that!(&adapted.body, json_equals("$.label.en", "Store"));
    expect_that!(&adapted.body, json_equals("$.label.de", "Laden"));
    expect_that!(&adapted.body, json_absent("$.label.languageMap"));
    expect_that!(&adapted.body, json_equals("$.category", "Building"));
    expect_that!(&adapted.body, json_equals("$.config.a", 1));
    expect_that!(&adapted.body, json_absent("$.config.json"));
    expect_that!(&adapted.body, json_count("$.tags", 2));
    expect_that!(&adapted.body, json_equals("$.tags[0]", "a"));
    expect_that!(&adapted.body, json_equals("$.tags[1]", "b"));
    expect_that!(
        &adapted.body,
        json_equals("$.refBuilding.object", format!("urn:ngsi-ld:Building:{namespace}-target"))
    );
    expect_that!(&adapted.body, json_absent("$.refBuilding.type"));
    expect_that!(&adapted.body, json_absent("$.refBuilding.objectType"));
    expect_that!(&adapted.body, json_absent("$.refBuilding.observedAt"));
    expect_that!(&adapted.body, json_equals("$.pressure", 30));
    Ok(())
}
