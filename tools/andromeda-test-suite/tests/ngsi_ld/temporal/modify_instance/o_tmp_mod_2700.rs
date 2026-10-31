//! A dataset id parameter is invalid request.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, instance_id_at},
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::problem,
    wire::api_uri,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::ModifyAttributeInstance},
    model::{Attribute, AttributeName, ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Modify Attribute instance with a `datasetId` URL parameter is refused with 400 `InvalidRequest` and
/// modifies nothing
/// Clause: 6.3.20, 6.22.3.1
/// Recorded: Modify Attribute instance defines no `datasetId` URL parameter; clause 6.3.20 says an incompatible
/// parameter "should" be answered with `InvalidRequest`
#[gtest]
#[tokio::test]
async fn o_tmp_mod_2700_a_dataset_id_parameter_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    let instance_id = instance_id_at(evolution, &"speed".parse()?, 0)?;
    let instance = Attribute::from(Property::builder(129).observed_at("2020-09-01T12:03:00Z".parse()?).build()?);
    // The operation defines no `datasetId` parameter; the request carries it as written.
    let request = broker
        .unchecked(
            &ModifyAttributeInstance::builder()
                .id(&vehicle_id)
                .name(&"speed".parse::<AttributeName>()?)
                .instance_id(&instance_id)
                .instance(&instance)
                .build(),
        )?
        .uri(api_uri(
            &broker,
            &format!("temporal/entities/{vehicle_id}/attrs/speed/{instance_id}?datasetId=urn:ngsi-ld:Dataset:{namespace}-fuel"),
        )?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(evolution_2, has_evolution("speed", nth(0, any_attribute().value(120))));
    Ok(())
}
