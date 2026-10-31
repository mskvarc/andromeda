//! Version 1 0 gives every simplified family its clause 4 5 4 shape.

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
        operation::{RepresentationSpelling, RetrieveEntity, Simplified},
    },
    model::NgsiLdMediaType,
};

/// Pins: Retrieve Entity with `options=keyValues` answers the `languageMap`, vocab and json wrappers and a dataset
/// map, and with Prefer ngsi-ld=1.0 answers 203 with each wrapper unwrapped to the bare value, the list families as
/// their arrays, the default instance alone, the first type alone and no scope or `expiresAt`
/// Clause: 4.3.6.8, 4.5.4, 6.3.6
#[gtest]
#[tokio::test]
async fn n_cmn_prf_9001_version_1_0_gives_every_simplified_family_its_clause_4_5_4_shape() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = create(&broker, &namespace, "simplified-families", LabelInstances::One).await?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let key_values = RetrieveEntity::builder()
        .id(&vehicle_id)
        .format(Simplified)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .build();
    // The answers are adapted to an older version, so their wire form is held as received.
    let current = send_versioned(&json, &key_values, None).await?;
    expect_that!(current.status, eq(StatusCode::OK));
    expect_that!(&current.body, json_equals("$.label.languageMap.en", "Store"));
    expect_that!(&current.body, json_equals("$.label.languageMap.de", "Laden"));
    expect_that!(&current.body, json_equals("$.category.vocab", "Building"));
    expect_that!(&current.body, json_equals("$.config.json.a", 1));
    expect_that!(&current.body, json_equals("$.pressure.dataset['@none']", 30));
    expect_that!(
        &current.body,
        json_equals(&format!("$.pressure.dataset['urn:ngsi-ld:Dataset:{namespace}-secondary']"), 25)
    );
    let adapted = send_versioned(&json, &key_values, Some("1.0")).await?;
    expect_that!(adapted.status, eq(StatusCode::NON_AUTHORITATIVE_INFORMATION));
    expect_that!(adapted.preference_applied.as_deref(), some(eq("ngsi-ld=1.0")));
    expect_that!(&adapted.body, json_equals("$.speed", 21));
    expect_that!(&adapted.body, json_equals("$.spot.type", "Point"));
    expect_that!(&adapted.body, json_equals("$.spot.coordinates[0]", 1.5));
    expect_that!(&adapted.body, json_equals("$.refBuilding", format!("urn:ngsi-ld:Building:{namespace}-target")));
    expect_that!(&adapted.body, json_equals("$.label.en", "Store"));
    expect_that!(&adapted.body, json_equals("$.label.de", "Laden"));
    expect_that!(&adapted.body, json_absent("$.label.languageMap"));
    expect_that!(&adapted.body, json_equals("$.category", "Building"));
    expect_that!(&adapted.body, json_equals("$.config.a", 1));
    expect_that!(&adapted.body, json_absent("$.config.json"));
    expect_that!(&adapted.body, json_count("$.tags", 2));
    expect_that!(&adapted.body, json_equals("$.tags[0]", "a"));
    expect_that!(&adapted.body, json_equals("$.tags[1]", "b"));
    expect_that!(&adapted.body, json_count("$.refRooms", 2));
    expect_that!(&adapted.body, json_equals("$.refRooms[0]", format!("urn:ngsi-ld:Room:{namespace}-first")));
    expect_that!(&adapted.body, json_equals("$.refRooms[1]", format!("urn:ngsi-ld:Room:{namespace}-second")));
    expect_that!(&adapted.body, json_equals("$.pressure", 30));
    expect_that!(&adapted.body, json_absent("$.scope"));
    expect_that!(&adapted.body, json_absent("$.expiresAt"));
    expect_that!(&adapted.body, json_equals("$.type", format!("Vehicle_{namespace}")));
    Ok(())
}
