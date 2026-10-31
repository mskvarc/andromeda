//! A simplified dataset map is reduced to its default instance below 1 3.

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

/// Pins: Retrieve Entity with `options=keyValues` answers with Prefer ngsi-ld=1.0 the default instance alone of
/// each multi-instance attribute, and with Prefer ngsi-ld=1.3 the dataset map keyed by @none and the `datasetId`,
/// each instance adapted in its own right
/// Clause: 4.3.6.8, 4.5.4, 4.5.5
#[gtest]
#[tokio::test]
async fn n_cmn_prf_9004_a_simplified_dataset_map_is_reduced_to_its_default_instance_below_1_3() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = create(&broker, &namespace, "simplified-dataset", LabelInstances::Datasets).await?;
    let secondary = format!("urn:ngsi-ld:Dataset:{namespace}-secondary");
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let key_values = RetrieveEntity::builder()
        .id(&vehicle_id)
        .format(Simplified)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .build();
    // The answers are adapted to an older version, so their wire form is held as received.
    let version_1_0 = send_versioned(&json, &key_values, Some("1.0")).await?;
    expect_that!(version_1_0.status, eq(StatusCode::NON_AUTHORITATIVE_INFORMATION));
    expect_that!(version_1_0.preference_applied.as_deref(), some(eq("ngsi-ld=1.0")));
    expect_that!(&version_1_0.body, json_equals("$.pressure", 30));
    expect_that!(&version_1_0.body, json_equals("$.label.en", "Depot"));
    expect_that!(&version_1_0.body, json_absent("$.label.dataset"));
    let version_1_3 = send_versioned(&json, &key_values, Some("1.3")).await?;
    expect_that!(version_1_3.status, eq(StatusCode::NON_AUTHORITATIVE_INFORMATION));
    expect_that!(version_1_3.preference_applied.as_deref(), some(eq("ngsi-ld=1.3")));
    expect_that!(&version_1_3.body, json_equals("$.pressure.dataset['@none']", 30));
    expect_that!(&version_1_3.body, json_equals(&format!("$.pressure.dataset['{secondary}']"), 25));
    expect_that!(&version_1_3.body, json_equals("$.label.dataset['@none'].en", "Depot"));
    expect_that!(&version_1_3.body, json_equals(&format!("$.label.dataset['{secondary}'].en"), "Store"));
    expect_that!(&version_1_3.body, json_absent("$.label.dataset['@none'].languageMap"));
    Ok(())
}
