//! A simplified family is unwrapped only below the version that introduced it.

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
        operation::{RepresentationSpelling, RetrieveEntity, Simplified},
    },
    model::NgsiLdMediaType,
};

/// Pins: Retrieve Entity with `options=keyValues` answers with Prefer ngsi-ld=1.4 the `LanguageProperty` wrapper,
/// the scope and the dataset map kept and the `VocabProperty` and `JsonProperty` unwrapped, and with Prefer
/// ngsi-ld=1.8 every wrapper kept and no `expiresAt`
/// Clause: 4.3.6.8, 4.5.4
#[gtest]
#[tokio::test]
async fn n_cmn_prf_9003_a_simplified_family_is_unwrapped_only_below_the_version_that_introduced_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = create(&broker, &namespace, "simplified-thresholds", LabelInstances::One).await?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let key_values = RetrieveEntity::builder()
        .id(&vehicle_id)
        .format(Simplified)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .build();
    // The answers are adapted to an older version, so their wire form is held as received.
    let version_1_4 = send_versioned(&json, &key_values, Some("1.4")).await?;
    expect_that!(version_1_4.status, eq(StatusCode::NON_AUTHORITATIVE_INFORMATION));
    expect_that!(version_1_4.preference_applied.as_deref(), some(eq("ngsi-ld=1.4")));
    expect_that!(&version_1_4.body, json_equals("$.label.languageMap.en", "Store"));
    expect_that!(&version_1_4.body, json_equals("$.label.languageMap.de", "Laden"));
    expect_that!(&version_1_4.body, json_equals("$.category", "Building"));
    expect_that!(&version_1_4.body, json_equals("$.config.a", 1));
    expect_that!(&version_1_4.body, json_absent("$.config.json"));
    expect_that!(&version_1_4.body, json_equals("$.scope", "/conformance"));
    expect_that!(&version_1_4.body, json_equals("$.pressure.dataset['@none']", 30));
    expect_that!(
        &version_1_4.body,
        json_equals(&format!("$.pressure.dataset['urn:ngsi-ld:Dataset:{namespace}-secondary']"), 25)
    );
    let version_1_8 = send_versioned(&json, &key_values, Some("1.8")).await?;
    expect_that!(version_1_8.status, eq(StatusCode::NON_AUTHORITATIVE_INFORMATION));
    expect_that!(version_1_8.preference_applied.as_deref(), some(eq("ngsi-ld=1.8")));
    expect_that!(&version_1_8.body, json_equals("$.label.languageMap.en", "Store"));
    expect_that!(&version_1_8.body, json_equals("$.label.languageMap.de", "Laden"));
    expect_that!(&version_1_8.body, json_equals("$.category.vocab", "Building"));
    expect_that!(&version_1_8.body, json_equals("$.config.json.a", 1));
    expect_that!(&version_1_8.body, json_absent("$.expiresAt"));
    Ok(())
}
