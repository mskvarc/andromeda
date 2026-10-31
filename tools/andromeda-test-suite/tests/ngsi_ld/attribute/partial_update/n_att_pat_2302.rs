//! An observed at with a comma fraction is accepted.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Partial Attribute Update accepts an observedAt whose seconds fraction is separated by a comma
/// Clause: 4.6.3, 5.6.4
#[gtest]
#[tokio::test]
async fn n_att_pat_2302_an_observed_at_with_a_comma_fraction_is_accepted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // The model writes a comma fraction with a point, and the comma is the subject, so the patch is sent as written.
    let request = json_request_to(
        &broker,
        Method::PATCH,
        &format!("entities/{vehicle_id}/attrs/speed"),
        r#"{"value": 2, "observedAt": "2024-01-01T00:00:00,5Z"}"#,
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_attribute("speed", property().value(2).observed_at("2024-01-01T00:00:00.5Z".parse()?))
    );
    Ok(())
}
