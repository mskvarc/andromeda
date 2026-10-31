//! Version 1 0 gives a simplified query the clause 4 5 4 shape.

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
        operation::{QueryEntities, RepresentationSpelling, Simplified},
    },
    model::NgsiLdMediaType,
    query::builders::SelectorTypeBuilder,
};

/// Pins: Query Entities with `options=keyValues` and Prefer ngsi-ld=1.0 answers 203 with the one matching Entity
/// unwrapped, reduced to its default instance and without scope
/// Clause: 4.3.6.8, 4.5.4, 5.7.2
#[gtest]
#[tokio::test]
async fn n_cmn_prf_9008_version_1_0_gives_a_simplified_query_the_clause_4_5_4_shape() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = create(&broker, &namespace, "simplified-query", LabelInstances::One).await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let selector = SelectorTypeBuilder::from(&vehicle_type).build()?;
    let ids = [vehicle_id];
    let query = QueryEntities::builder()
        .entity_type(&selector)
        .ids(&ids)
        .format(Simplified)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .build();
    // The answer is adapted to an older version, so its wire form is held as received.
    let answer = send_versioned(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), &query, Some("1.0")).await?;
    expect_that!(answer.status, eq(StatusCode::NON_AUTHORITATIVE_INFORMATION));
    expect_that!(answer.preference_applied.as_deref(), some(eq("ngsi-ld=1.0")));
    expect_that!(&answer.body, json_count("$", 1));
    expect_that!(&answer.body, json_equals("$[0].label.en", "Store"));
    expect_that!(&answer.body, json_equals("$[0].category", "Building"));
    expect_that!(&answer.body, json_equals("$[0].config.a", 1));
    expect_that!(&answer.body, json_equals("$[0].pressure", 30));
    expect_that!(&answer.body, json_absent("$[0].scope"));
    Ok(())
}
