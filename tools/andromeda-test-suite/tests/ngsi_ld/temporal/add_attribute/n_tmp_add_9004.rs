//! A reified scope outside the grammar is refused on append.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, problem::problem, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{
        ErrorType,
        NgsiLdMediaType,
        Property,
        Scope,
        evolution::{ScopeInstance, TemporalEntity},
    },
};

/// Pins: Add Attributes to Temporal Evolution of an Entity with a scope instance holding /a-b answers 400
/// `BadRequestData`, and the Temporal Evolution reads back as before
/// Clause: 4.18, 4.5.6, 5.5.4, 5.6.12.4
#[gtest]
#[tokio::test]
async fn n_tmp_add_9004_a_reified_scope_outside_the_grammar_is_refused_on_append() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", "reified-append")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let observed = "2020-06-01T00:00:00Z".parse()?;
    let vehicle = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .scope(ScopeInstance::new("/Madrid".parse::<Scope>()?).with_observed_at(observed))
        .property("speed", Property::builder(80).observed_at(observed))
        .build()?;
    broker.temporal().create_or_update(&vehicle).send().await?.created()?;
    let json = broker.with_responses().with_accept(Accept::new(NgsiLdMediaType::Json));
    let before = json.temporal().retrieve(&vehicle_id).send().await?;
    // The model has no scope outside the grammar, so the instance is sent as written.
    let refused = broker
        .send_unchecked(json_request_to(
            &broker,
            Method::POST,
            &format!("temporal/entities/{vehicle_id}/attrs"),
            r#"{"scope": [{"type": "Property", "value": "/a-b", "observedAt": "2020-06-01T00:00:00Z"}]}"#,
        )?)
        .await;
    expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let after = json.temporal().retrieve(&vehicle_id).send().await?;
    expect_that!(after.body, eq(&before.body));
    Ok(())
}
