//! A create entity map under an older version preference is answered 203 with the preference applied.

use andromeda_test_suite::{broker, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{
    http::{
        headers::{Prefer, SpecVersion},
        operation::CreateEntityMap,
    },
    query::builders::SelectorTypeBuilder,
};

/// Pins: Create `EntityMap` for Query Entities through GET with Prefer ngsi-ld=1.8 answers 203 with a
/// Preference-Applied header naming a version
/// Clause: 6.34.3.1, 6.3.6, 4.3.6.8
#[gtest]
#[tokio::test]
async fn n_cmn_prf_7200_a_create_entity_map_under_an_older_version_preference_is_answered_203_with_the_preference_applied() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let selector = SelectorTypeBuilder::from(&vehicle_type).build()?;
    let creation = CreateEntityMap::builder().entity_type(&selector).build();
    // An entity map adapted to version 1.8 holds no `expiresAt`, which the model requires, so the answer is read as a
    // response.
    let request = broker
        .unchecked(&creation)?
        .typed_header(Prefer::builder().version("1.8".parse::<SpecVersion>()?).build()?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        answered(StatusCode::NON_AUTHORITATIVE_INFORMATION).with_header_containing(HeaderName::from_static("preference-applied"), "ngsi-ld=")
    );
    Ok(())
}
