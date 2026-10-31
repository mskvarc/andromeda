//! No version gives a simplified attribute a value wrapper.

use andromeda_test_suite::{
    body_json::json_absent,
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

/// Pins: Retrieve Entity with `options=keyValues` and with format=simplified, each with Prefer ngsi-ld=1.0, 1.3,
/// 1.4 and 1.8, answers 203 with no attribute rendered as an object carrying a value member
/// Clause: 4.3.6.8, 4.5.4
#[gtest]
#[tokio::test]
async fn n_cmn_prf_9002_no_version_gives_a_simplified_attribute_a_value_wrapper() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = create(&broker, &namespace, "simplified-no-wrapper", LabelInstances::One).await?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let spellings = [RepresentationSpelling::default().in_options().key_values(), RepresentationSpelling::default()];
    for spelling in spellings {
        let retrieve = RetrieveEntity::builder().id(&vehicle_id).format(Simplified).spelling(spelling).build();
        for version in ["1.0", "1.3", "1.4", "1.8"] {
            // The answers are adapted to an older version, so their wire form is held as received.
            let answer = send_versioned(&json, &retrieve, Some(version)).await?;
            expect_that!(answer.status, eq(StatusCode::NON_AUTHORITATIVE_INFORMATION));
            let applied = format!("ngsi-ld={version}");
            expect_that!(answer.preference_applied.as_deref(), some(eq(applied.as_str())));
            expect_that!(&answer.body, json_absent("$.*.value"));
        }
    }
    Ok(())
}
