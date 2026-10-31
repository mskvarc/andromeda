//! The type options and lang parameters are accepted and the simplified fragment is merged.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    wire::api_uri,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::MergeSimplifiedEntity},
    model::{Entity, SimplifiedFragment},
    query::Locality,
};

/// Pins: Merge Entity with the parameters type, options `keyValues` and lang and a simplified fragment answers 204
/// and the merged value reads back as a Property
/// Clause: 6.5.3.4, 5.6.17.4, 4.5.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_7201_the_type_options_and_lang_parameters_are_accepted_and_the_simplified_fragment_is_merged() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = SimplifiedFragment::builder().value("speed", 99).build()?;
    // The operation writes `format=simplified`; `options=keyValues` is its synonym and goes out as written.
    let request = broker
        .unchecked(&MergeSimplifiedEntity::builder().id(&vehicle_id).fragment(&fragment).build())?
        .uri(api_uri(
            &broker,
            &format!("entities/{vehicle_id}?type={vehicle_type}&options=keyValues&lang=en"),
        )?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(retrieved.body(), has_attribute("speed", any_attribute().value(99)));
    Ok(())
}
