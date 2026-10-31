//! A reified scope that is no property is refused.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateOrUpdateTemporalEvolution},
    model::{ErrorType, Lossless, Property, RawJson, evolution::TemporalEntity},
};

/// Pins: Create Temporal Evolution of an Entity whose reified scope is a Relationship answers 400 `BadRequestData`
/// Clause: 4.5.6, 5.5.4
#[gtest]
#[tokio::test]
async fn n_tmp_ups_9015_a_reified_scope_that_is_no_property_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_reified_relationship_id = namespace.urn("Vehicle", "reified-relationship")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_reified_relationship_evolution = TemporalEntity::builder()
        .id(vehicle_reified_relationship_id)
        .entity_type(vehicle_type)
        .property("speed", Property::builder(80).observed_at("2020-06-01T00:00:00Z".parse()?))
        .build()?;
    // The model refuses it (the evolution of a scope is a Property, found type `Relationship` at scope), so `scope` is written as text.
    let scope: RawJson =
        format!(r#"[{{"type": "Relationship", "object": "urn:ngsi-ld:Scope:{namespace}-madrid", "observedAt": "2020-06-01T00:00:00Z"}}]"#).parse()?;
    let document = UncheckedDocument::try_from(vehicle_reified_relationship_evolution.render(Lossless::Normalized))?.set_member("scope", &scope);
    let request = broker
        .unchecked(
            &CreateOrUpdateTemporalEvolution::builder()
                .entity(&vehicle_reified_relationship_evolution)
                .build(),
        )?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
