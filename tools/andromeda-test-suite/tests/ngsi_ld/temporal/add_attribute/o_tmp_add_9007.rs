//! Every reified scope instance added is recorded as given.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, problem::answered, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Two Add Attributes to Temporal Evolution requests each carrying a reified scope /<ns>/A, observed at
/// 2024-01-01T00:00:00Z and at 00:30, leave two scope instances in the Temporal Evolution, one at each instant
/// Clause: 4.18, 4.5.6, 4.5.7, 5.6.12.4
/// Recorded: clauses 5.6.11.4 and 5.6.12.4 do not say what a scope member of a Temporal Evolution Fragment records;
/// Andromeda records a reified scope instance as given, dated by its own `observedAt`
#[gtest]
#[tokio::test]
async fn o_tmp_add_9007_every_reified_scope_instance_added_is_recorded_as_given() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", "scope-instances")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:10:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle).send().await?.created()?;
    for observed_at in ["2024-01-01T00:00:00Z", "2024-01-01T00:30:00Z"] {
        // The scope is a single instance object, which Add Attributes to Temporal Evolution does not write, so the body is sent as written.
        let appended = broker
            .send_unchecked(json_request_to(
                &broker,
                Method::POST,
                &format!("temporal/entities/{vehicle_id}/attrs"),
                &format!(r#"{{"scope": {{"type": "Property", "value": "/{namespace}/A", "observedAt": "{observed_at}"}}}}"#),
            )?)
            .await;
        expect_that!(&appended, answered(StatusCode::NO_CONTENT));
    }
    let evolution = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let observed = evolution
        .body()
        .scope
        .iter()
        .map(|instance| instance.observed_at().copied())
        .collect::<Vec<_>>();
    expect_that!(
        &observed,
        eq(&vec![
            Some("2024-01-01T00:00:00Z".parse::<DateTime>()?),
            Some("2024-01-01T00:30:00Z".parse::<DateTime>()?)
        ])
    );
    Ok(())
}
